use std::{collections::{HashMap, HashSet}, net::SocketAddr, sync::Arc};

use anyhow::Context;
use axum::{
    extract::{Path, Query, State, WebSocketUpgrade},
    http::{self, header, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use axum::extract::ws::{Message, WebSocket};
// no get_service; serve files via handlers to avoid trait bound issues
use base64::Engine;
use dashmap::DashMap;
use futures::{SinkExt, StreamExt};
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use rand::RngCore;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokio::sync::mpsc;
use tokio::fs;
use tokio::net::TcpListener;
use tower_http::{cors::{Any, CorsLayer}, trace::TraceLayer};
use tracing::info;
use uuid::Uuid;

static HMAC_KEY: OnceCell<[u8; 32]> = OnceCell::new();

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // init tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tower_http=info".into()),
        )
        .init();

    // initialize HMAC key
    let key_bytes = std::env::var("ZOBBO_HMAC_KEY")
        .ok()
        .and_then(|hex| hex::decode(hex).ok())
        .and_then(|v| v.try_into().ok())
        .unwrap_or_else(|| {
            let mut kb = [0u8; 32];
            rand::thread_rng().fill_bytes(&mut kb);
            kb
        });
    HMAC_KEY.set(key_bytes).ok();

    let state = AppState::default();

    let api = Router::new()
        .route("/healthz", get(health))
        .route("/api/room", post(create_room))
        .route("/api/room/:room_id/join", post(join_room))
        .route("/api/room/:room_id/ws", get(ws_handler));

    let app = Router::new()
        .merge(api)
        // Serve core assets
        .route("/app.js", get(serve_app_js))
        .route("/main.js", get(serve_main_js))
        .route("/store.js", get(serve_store_js))
        .route("/gameStore.js", get(serve_gamestore_js))
        .route("/components/:file", get(serve_component_js))
        .route("/favicon.svg", get(serve_favicon))
        // SPA index fallback
        .route("/", get(index_html))
        .route("/*path", get(index_html))
        .layer(
            CorsLayer::new()
                .allow_methods([http::Method::GET, http::Method::POST])
                .allow_headers([header::CONTENT_TYPE])
                .allow_origin(Any),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let port: u16 = std::env::var("PORT").ok().and_then(|s| s.parse().ok()).unwrap_or(8080);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("listening on http://{}", addr);
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

// Map errors to 500 for JSON endpoints
fn internal_error<E: std::fmt::Display>(err: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}

#[derive(Clone, Default)]
struct AppState {
    rooms: Arc<DashMap<Uuid, Arc<Room>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")] 
enum GameMode {
    SuddenDeath,
    ZobboBattle { rounds: u8 },
}

impl Default for GameMode {
    fn default() -> Self { GameMode::ZobboBattle { rounds: 3 } }
}

#[derive(Debug)]
struct Room {
    id: Uuid,
    created_at: OffsetDateTime,
    mode: GameMode,
    started: Mutex<bool>,
    // Players by id
    players: Mutex<HashMap<Uuid, PlayerRecord>>,
    game: Mutex<Option<GameState>>,
    // Max 2 players
}

#[derive(Debug)]
struct PlayerRecord {
    name: String,
    connected: bool,
    ready: bool,
    tx: Option<mpsc::UnboundedSender<ServerToClient>>, // for server push
}

#[derive(Debug, Serialize, Deserialize)]
struct CreateRoomRequest {
    #[serde(default)]
    mode: GameMode,
}

#[derive(Debug, Serialize, Deserialize)]
struct CreateRoomResponse {
    room_id: Uuid,
    share_url: String,
}

async fn health() -> &'static str { "ok" }

async fn create_room(
    State(state): State<AppState>,
    Json(req): Json<CreateRoomRequest>,
) -> Result<Json<CreateRoomResponse>, (StatusCode, String)> {
    let room_id = Uuid::new_v4();
    let room = Arc::new(Room {
        id: room_id,
        created_at: OffsetDateTime::now_utc(),
        mode: req.mode,
        started: Mutex::new(false),
        players: Mutex::new(HashMap::new()),
        game: Mutex::new(None),
    });
    state.rooms.insert(room_id, room);

    let host = std::env::var("HOST_PUBLIC_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    Ok(Json(CreateRoomResponse {
        room_id,
        share_url: format!("{}/{}", host.trim_end_matches('/'), room_id),
    }))
}

#[derive(Debug, Deserialize)]
struct JoinPath {
    room_id: Uuid,
}

#[derive(Debug, Serialize, Deserialize)]
struct JoinRoomRequest { name: String }

#[derive(Debug, Serialize, Deserialize)]
struct JoinRoomResponse { token: String, player_id: Uuid }

async fn join_room(
    State(state): State<AppState>,
    Path(JoinPath { room_id }): Path<JoinPath>,
    Json(req): Json<JoinRoomRequest>,
) -> Result<Json<JoinRoomResponse>, (StatusCode, String)> {
    let Some(room) = state.rooms.get(&room_id).map(|r| r.clone()) else {
        return Err((StatusCode::NOT_FOUND, "Room not found".into()));
    };

    let mut players = room.players.lock();
    if players.len() >= 2 {
        return Err((StatusCode::CONFLICT, "Room is full".into()));
    }
    // create player id
    let player_id = Uuid::new_v4();
    players.insert(player_id, PlayerRecord { name: req.name, connected: false, ready: false, tx: None });
    drop(players);

    let token = issue_token(room_id, player_id).map_err(internal_error)?;
    Ok(Json(JoinRoomResponse { token, player_id }))
}

#[derive(Debug, Deserialize)]
struct WsQuery { token: String }

async fn ws_handler(
    State(state): State<AppState>,
    Path(JoinPath { room_id }): Path<JoinPath>,
    Query(WsQuery { token }): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let (tok_room, player_id) = verify_token(&token).map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid token".into()))?;
    if tok_room != room_id {
        return Err((StatusCode::UNAUTHORIZED, "Token-room mismatch".into()));
    }
    let Some(room) = state.rooms.get(&room_id).map(|r| r.clone()) else {
        return Err((StatusCode::NOT_FOUND, "Room not found".into()));
    };
    // ensure player exists
    {
        let players = room.players.lock();
        if !players.contains_key(&player_id) { return Err((StatusCode::UNAUTHORIZED, "Unknown player".into())); }
    }

    Ok(ws.on_upgrade(move |socket| handle_socket(room, player_id, socket)))
}

async fn handle_socket(room: Arc<Room>, player_id: Uuid, socket: WebSocket) {
    let (ws_tx, mut ws_rx) = socket.split();
    // channel for server -> client messages
    let (sv_tx, mut sv_rx) = mpsc::unbounded_channel::<ServerToClient>();

    // mark connected and store tx
    {
        let mut players = room.players.lock();
        if let Some(p) = players.get_mut(&player_id) {
            p.connected = true;
            p.tx = Some(sv_tx.clone());
        }
    }

    // broadcast lobby update on connect
    let lobby = LobbyState::from_room(&room);
    broadcast(&room, &ServerToClient::LobbyUpdate { lobby });

    // Spawn a task to forward server messages to websocket
    tokio::spawn(async move {
        let mut ws_tx = ws_tx;
        while let Some(msg) = sv_rx.recv().await {
            let text = serde_json::to_string(&msg).unwrap();
            if ws_tx.send(Message::Text(text)).await.is_err() {
                break;
            }
        }
    });

    // Send welcome and lobby state
    let lobby = LobbyState::from_room(&room);
    let _ = sv_tx.send(ServerToClient::Welcome { player_id, lobby });

    // Read client messages
    while let Some(Ok(msg)) = ws_rx.next().await {
        match msg {
            Message::Text(txt) => {
                match serde_json::from_str::<ClientToServer>(&txt) {
                    Ok(ClientToServer::Ping) => {
                        let _ = sv_tx.send(ServerToClient::Pong);
                    }
                    Ok(ClientToServer::Ready) => {
                        on_player_ready(&room, player_id).await;
                    }
                    Ok(ClientToServer::DrawDeck) => { let _ = handle_draw(&room, player_id, DrawSource::Deck); }
                    Ok(ClientToServer::DrawDiscard) => { let _ = handle_draw(&room, player_id, DrawSource::Discard); }
                    Ok(ClientToServer::SwapWithHand { index }) => { let _ = handle_swap_with_hand(&room, player_id, index); }
                    Ok(ClientToServer::DiscardDrawn) => { let _ = handle_discard_drawn(&room, player_id); }
                    Ok(ClientToServer::MatchTop { index }) => { let _ = handle_match_top(&room, player_id, index); }
                    Ok(ClientToServer::CallZobbo) => { let _ = handle_call_zobbo(&room, player_id); }
                    Ok(ClientToServer::PowerCheckOwn { index }) => { let _ = handle_power_check_own(&room, player_id, index); }
                    Ok(ClientToServer::PowerCheckOpp { index }) => { let _ = handle_power_check_opp(&room, player_id, index); }
                    Ok(ClientToServer::PowerSwapWithDeck { index }) => { let _ = handle_power_swap_with_deck(&room, player_id, index); }
                    Ok(ClientToServer::PowerSwapWithOpp { my_index, opp_index }) => { let _ = handle_power_swap_with_opp(&room, player_id, my_index, opp_index); }
                    Ok(ClientToServer::PowerOppSwapWithDeck { opp_index }) => { let _ = handle_power_opp_swap_with_deck(&room, player_id, opp_index); }
                    Ok(ClientToServer::EndTurn) => { let _ = handle_end_turn(&room, player_id); }
                    Err(err) => {
                        let _ = sv_tx.send(ServerToClient::Error { message: format!("Bad message: {}", err) });
                    }
                }
            }
            Message::Close(_) => break,
            Message::Binary(_) => {}
            Message::Ping(_) => {}
            Message::Pong(_) => {}
        }
    }

    // mark disconnected
    {
        let mut players = room.players.lock();
        if let Some(p) = players.get_mut(&player_id) {
            p.connected = false;
            p.tx = None;
        }
    }

    // broadcast lobby update on disconnect
    let lobby = LobbyState::from_room(&room);
    broadcast(&room, &ServerToClient::LobbyUpdate { lobby });
}

async fn on_player_ready(room: &Arc<Room>, player_id: Uuid) {
    // Mark this player as ready
    let mut maybe_start: Option<Uuid> = None;
    {
        let mut players = room.players.lock();
        if let Some(p) = players.get_mut(&player_id) { p.ready = true; }
        let all: Vec<(Uuid, bool, bool)> = players.iter().map(|(id, p)| (*id, p.connected, p.ready)).collect();
        if all.len() == 2 && all.iter().all(|(_, c, r)| *c && *r) {
            let mut started = room.started.lock();
            if !*started {
                *started = true;
                // pick random starting player
                let start_id = if rand::random::<bool>() { all[0].0 } else { all[1].0 };
                maybe_start = Some(start_id);
                // initialize game state now
                start_game(room, start_id);
            }
        }
    }
    // broadcast lobby update (ready status)
    let lobby = LobbyState::from_room(room);
    broadcast(room, &ServerToClient::LobbyUpdate { lobby });

    // if we just started, broadcast GameStart
    if let Some(starting_player) = maybe_start {
        broadcast(room, &ServerToClient::GameStart { starting_player, mode: room.mode.clone() });
        // send initial game update and peeks
        broadcast_game_update(room);
        send_initial_peeks(room);
    }
}

fn broadcast(room: &Arc<Room>, msg: &ServerToClient) {
    let players = room.players.lock();
    for (_pid, p) in players.iter() {
        if let Some(tx) = &p.tx {
            let _ = tx.send(msg.clone());
        }
    }
}

fn send_to(room: &Arc<Room>, player: Uuid, msg: &ServerToClient) {
    let players = room.players.lock();
    if let Some(p) = players.get(&player) {
        if let Some(tx) = &p.tx { let _ = tx.send(msg.clone()); }
    }
}

// ========== Game helpers ==========
fn rank_points(c: &Card) -> u32 {
    match c.rank {
        Rank::Ace => 1,
        Rank::Two => 2,
        Rank::Three => 3,
        Rank::Four => 4,
        Rank::Five => 5,
        Rank::Six => 6,
        Rank::Seven => 7,
        Rank::Eight => 8,
        Rank::Nine => 9,
        Rank::Ten => 10,
        Rank::Jack => 11,
        Rank::Queen => 12,
        Rank::King => if c.suit.is_red() { 13 } else { 0 },
    }
}

fn card_public(c: &Card) -> CardPublic {
    CardPublic { rank: c.rank, is_red_king: matches!(c.rank, Rank::King) && c.suit.is_red() }
}

fn card_public_full(c: &Card) -> CardPublicFull { CardPublicFull { rank: c.rank, suit: c.suit } }

fn build_deck() -> Vec<Card> {
    let suits = [Suit::Clubs, Suit::Diamonds, Suit::Hearts, Suit::Spades];
    let ranks = [
        Rank::Ace, Rank::Two, Rank::Three, Rank::Four, Rank::Five, Rank::Six,
        Rank::Seven, Rank::Eight, Rank::Nine, Rank::Ten, Rank::Jack, Rank::Queen, Rank::King,
    ];
    let mut deck = Vec::with_capacity(52);
    for s in suits { for r in ranks { deck.push(Card { rank: r, suit: s }); } }
    deck.shuffle(&mut rand::thread_rng());
    deck
}

fn ensure_deck(game: &mut GameState) {
    if game.deck.is_empty() && game.discard.len() > 1 {
        // keep top of discard, shuffle the rest back into deck
        let top = game.discard.pop().unwrap();
        let mut rest = std::mem::take(&mut game.discard);
        rest.shuffle(&mut rand::thread_rng());
        game.deck = rest;
        game.discard.push(top);
    }
}

fn draw_from_deck(game: &mut GameState) -> Option<Card> {
    ensure_deck(game);
    game.deck.pop()
}

fn get_both_ids(room: &Arc<Room>) -> Option<(Uuid, Uuid)> {
    let players = room.players.lock();
    let ids: Vec<Uuid> = players.keys().copied().collect();
    if ids.len() == 2 { Some((ids[0], ids[1])) } else { None }
}

fn opponent_of(game: &GameState, me: Uuid) -> Option<Uuid> {
    game.seats.keys().copied().find(|id| *id != me)
}

fn compose_seat_public(seat: &Seat) -> SeatPublic {
    let slots = seat.slots.iter().zip(seat.versions.iter()).map(|(s, v)| SlotPublic { empty: s.is_none(), version: *v }).collect();
    SeatPublic { slots }
}

fn compose_update_for(room: &Arc<Room>, pid: Uuid) -> Option<GameUpdate> {
    let game_opt = room.game.lock().clone();
    let Some(game) = game_opt else { return None; };
    let you = game.seats.get(&pid)?;
    let opp_id = opponent_of(&game, pid)?;
    let opp = game.seats.get(&opp_id)?;
    let discard_top = game.discard.last().map(|c| card_public(c));
    let stage = match &game.stage {
        TurnStage::AwaitDraw => "await_draw",
        TurnStage::Holding { .. } => "holding",
        TurnStage::Power { .. } => "power",
    }.to_string();
    Some(GameUpdate {
        you: compose_seat_public(you),
        opponent: OpponentPublic { slots: opp.slots.iter().zip(opp.versions.iter()).map(|(s,v)| SlotPublic { empty: s.is_none(), version: *v }).collect() },
        active: game.active,
        stage,
        discard_top,
        deck_count: game.deck.len() as u32,
        discard_count: game.discard.len() as u32,
        zobbo_remaining: game.zobbo.as_ref().map(|z| z.remaining),
    })
}

fn broadcast_game_update(room: &Arc<Room>) {
    let players: Vec<Uuid> = room.players.lock().keys().copied().collect();
    for pid in players {
        if let Some(update) = compose_update_for(room, pid) {
            send_to(room, pid, &ServerToClient::GameUpdate { update });
        }
    }
}

fn start_game(room: &Arc<Room>, starting: Uuid) {
    let Some((a,b)) = get_both_ids(room) else { return; };
    let mut deck = build_deck();
    let mut seats = HashMap::new();
    for pid in [a,b] {
        let mut slots = Vec::with_capacity(6);
        let versions = vec![0u64; 6];
        for _ in 0..6 { slots.push(deck.pop()); }
        let slots = slots.into_iter().map(|c| c).collect::<Option<Vec<_>>>();
        let slots = slots.unwrap_or_default();
        let seat = Seat { slots: slots.into_iter().map(Some).collect(), versions };
        seats.insert(pid, seat);
    }
    let gs = GameState { deck, discard: Vec::new(), seats, active: starting, stage: TurnStage::AwaitDraw, skip_next: HashSet::new(), zobbo: None };
    *room.game.lock() = Some(gs);
}

fn send_initial_peeks(room: &Arc<Room>) {
    let g_opt = room.game.lock().clone();
    let Some(game) = g_opt else { return; };
    for (&pid, seat) in game.seats.iter() {
        for idx in 0..3.min(seat.slots.len()) {
            if let Some(ref card) = seat.slots[idx] {
                send_to(room, pid, &ServerToClient::PeekResult { target: "self".into(), index: idx, card: card_public(card) });
            }
        }
    }
}

// ========== Action handlers ==========
fn is_active(room: &Arc<Room>, pid: Uuid) -> bool {
    room.game.lock().as_ref().map(|g| g.active == pid).unwrap_or(false)
}

fn next_player(game: &GameState) -> Option<Uuid> {
    game.seats.keys().copied().find(|id| *id != game.active)
}

fn handle_draw(room: &Arc<Room>, pid: Uuid, src: DrawSource) -> Result<(), ()> {
    let mut g_lock = room.game.lock();
    let g = match g_lock.as_mut() { Some(g) => g, None => return Err(()) };
    if g.active != pid { return Err(()); }
    if !matches!(g.stage, TurnStage::AwaitDraw) { return Err(()); }
    let card = match src {
        DrawSource::Deck => draw_from_deck(g).ok_or(())?,
        DrawSource::Discard => g.discard.pop().ok_or(())?,
    };
    g.stage = TurnStage::Holding { card, from: src };
    drop(g_lock);
    send_to(room, pid, &ServerToClient::DrawnCard { card: card_public(&card) });
    broadcast_game_update(room);
    Ok(())
}

fn end_turn_common(room: &Arc<Room>) {
    let mut g_lock = room.game.lock();
    let g = match g_lock.as_mut() { Some(g) => g, None => return };
    // advance active
    if let Some(mut next) = next_player(g) {
        if g.skip_next.remove(&next) {
            // skip opponent, keep current active
            next = g.active;
        }
        g.active = next;
    }
    g.stage = TurnStage::AwaitDraw;
    if let Some(z) = g.zobbo.as_mut() {
        if z.remaining > 0 { z.remaining -= 1; }
    }
    let zobbo_done = g.zobbo.as_ref().map(|z| z.remaining == 0).unwrap_or(false);
    drop(g_lock);
    broadcast_game_update(room);
    if zobbo_done { reveal_and_finish(room); }
}

fn handle_swap_with_hand(room: &Arc<Room>, pid: Uuid, index: usize) -> Result<(), ()> {
    let mut g_lock = room.game.lock();
    let g = match g_lock.as_mut() { Some(g) => g, None => return Err(()) };
    if g.active != pid { return Err(()); }
    let (holding_card, _) = match &g.stage { TurnStage::Holding { card, from } => (*card, *from), _ => return Err(()) };
    let seat = g.seats.get_mut(&pid).ok_or(())?;
    if index >= seat.slots.len() { return Err(()) }
    if seat.slots[index].is_none() { return Err(()) }
    let replaced = seat.slots[index].take().unwrap();
    seat.slots[index] = Some(holding_card);
    seat.versions[index] = seat.versions[index].wrapping_add(1);
    g.discard.push(replaced);
    g.stage = TurnStage::AwaitDraw; // will be reset in end_turn
    drop(g_lock);
    broadcast_game_update(room);
    end_turn_common(room);
    Ok(())
}

fn handle_discard_drawn(room: &Arc<Room>, pid: Uuid) -> Result<(), ()> {
    let mut g_lock = room.game.lock();
    let g = match g_lock.as_mut() { Some(g) => g, None => return Err(()) };
    if g.active != pid { return Err(()) }
    let (holding_card, from) = match &g.stage { TurnStage::Holding { card, from } => (*card, *from), _ => return Err(()) };
    g.discard.push(holding_card);
    // power only if from deck and rank in power set
    let power_allowed = matches!(from, DrawSource::Deck) && matches!(holding_card.rank, Rank::Five|Rank::Six|Rank::Seven|Rank::Eight|Rank::Nine|Rank::Ten|Rank::Jack|Rank::Queen|Rank::King);
    if power_allowed && !matches!(holding_card.rank, Rank::King) { // King power depends on red/black; only red K has power
        g.stage = TurnStage::Power { card: holding_card };
        drop(g_lock);
        broadcast_game_update(room);
        Ok(())
    } else if matches!(holding_card.rank, Rank::King) && holding_card.suit.is_red() {
        g.stage = TurnStage::Power { card: holding_card };
        drop(g_lock);
        broadcast_game_update(room);
        Ok(())
    } else {
        g.stage = TurnStage::AwaitDraw;
        drop(g_lock);
        broadcast_game_update(room);
        end_turn_common(room);
        Ok(())
    }
}

fn handle_match_top(room: &Arc<Room>, pid: Uuid, index: usize) -> Result<(), ()> {
    let mut g_lock = room.game.lock();
    let g = match g_lock.as_mut() { Some(g) => g, None => return Err(()) };
    let top_rank = match g.discard.last() { Some(c) => c.rank, None => return Err(()) };
    let seat = g.seats.get_mut(&pid).ok_or(())?;
    if index >= seat.slots.len() { return Err(()) }
    match &seat.slots[index] {
        Some(c) if c.rank == top_rank => {
            seat.slots[index] = None;
            seat.versions[index] = seat.versions[index].wrapping_add(1);
        }
        Some(_) => {
            // wrong match, skip next turn
            g.skip_next.insert(pid);
            drop(g_lock);
            broadcast_game_update(room);
            return Ok(());
        }
        None => { return Err(()) }
    }
    drop(g_lock);
    broadcast_game_update(room);
    Ok(())
}

fn handle_call_zobbo(room: &Arc<Room>, pid: Uuid) -> Result<(), ()> {
    let mut g_lock = room.game.lock();
    let g = match g_lock.as_mut() { Some(g) => g, None => return Err(()) };
    if !matches!(g.stage, TurnStage::AwaitDraw) { return Err(()) }
    if g.active == pid { return Err(()) } // only opponent may call before move
    if g.zobbo.is_none() { g.zobbo = Some(ZobboState { caller: pid, remaining: 2 }); }
    drop(g_lock);
    broadcast_game_update(room);
    Ok(())
}

fn handle_power_check_own(room: &Arc<Room>, pid: Uuid, index: usize) -> Result<(), ()> {
    let mut g_lock = room.game.lock();
    let g = match g_lock.as_mut() { Some(g) => g, None => return Err(()) };
    if g.active != pid { return Err(()) }
    let card = match &g.stage { TurnStage::Power { card } => *card, _ => return Err(()) };
    if !matches!(card.rank, Rank::Five|Rank::Six|Rank::Seven|Rank::Eight) { return Err(()) }
    let seat = g.seats.get(&pid).ok_or(())?;
    if index >= seat.slots.len() { return Err(()) }
    if let Some(c) = &seat.slots[index] {
        let peek = card_public(c);
        drop(g_lock);
        send_to(room, pid, &ServerToClient::PeekResult { target: "self".into(), index, card: peek });
        end_turn_common(room);
        Ok(())
    } else { Err(()) }
}

fn handle_power_check_opp(room: &Arc<Room>, pid: Uuid, index: usize) -> Result<(), ()> {
    let mut g_lock = room.game.lock();
    let g = match g_lock.as_mut() { Some(g) => g, None => return Err(()) };
    if g.active != pid { return Err(()) }
    let power_card = match &g.stage { TurnStage::Power { card } => *card, _ => return Err(()) };
    if !matches!(power_card.rank, Rank::Nine|Rank::Ten) { return Err(()) }
    let opp_id = opponent_of(g, pid).ok_or(())?;
    let seat = g.seats.get(&opp_id).ok_or(())?;
    if index >= seat.slots.len() { return Err(()) }
    if let Some(c) = &seat.slots[index] {
        let peek = card_public(c);
        drop(g_lock);
        send_to(room, pid, &ServerToClient::PeekResult { target: "opponent".into(), index, card: peek });
        end_turn_common(room);
        Ok(())
    } else { Err(()) }
}

fn handle_power_swap_with_deck(room: &Arc<Room>, pid: Uuid, index: usize) -> Result<(), ()> {
    let mut g_lock = room.game.lock();
    let g = match g_lock.as_mut() { Some(g) => g, None => return Err(()) };
    if g.active != pid { return Err(()) }
    let power_card = match &g.stage { TurnStage::Power { card } => *card, _ => return Err(()) };
    if !matches!(power_card.rank, Rank::Jack) { return Err(()) }
    let old_opt = {
        let seat = g.seats.get_mut(&pid).ok_or(())?;
        if index >= seat.slots.len() { return Err(()) }
        seat.slots[index].take()
    };
    if let Some(old) = old_opt {
        let new_opt = draw_from_deck(g);
        {
            let seat = g.seats.get_mut(&pid).ok_or(())?;
            if let Some(newc) = new_opt {
                seat.slots[index] = Some(newc);
                seat.versions[index] = seat.versions[index].wrapping_add(1);
                g.deck.push(old);
            } else {
                // restore if deck empty
                seat.slots[index] = Some(old);
            }
        }
    }
    drop(g_lock);
    broadcast_game_update(room);
    end_turn_common(room);
    Ok(())
}

fn handle_power_swap_with_opp(room: &Arc<Room>, pid: Uuid, my_index: usize, opp_index: usize) -> Result<(), ()> {
    let mut g_lock = room.game.lock();
    let g = match g_lock.as_mut() { Some(g) => g, None => return Err(()) };
    if g.active != pid { return Err(()) }
    let power_card = match &g.stage { TurnStage::Power { card } => *card, _ => return Err(()) };
    if !matches!(power_card.rank, Rank::Queen) { return Err(()) }
    let opp_id = opponent_of(g, pid).ok_or(())?;

    // Take my card out first; error if slot empty or OOB
    let my_card = {
        let seat_me = g.seats.get_mut(&pid).ok_or(())?;
        if my_index >= seat_me.slots.len() { return Err(()) }
        match seat_me.slots[my_index].take() { Some(c) => c, None => return Err(()) }
    };

    // Then take opponent card; if it fails, restore mine and error
    let opp_card = {
        let seat_opp = g.seats.get_mut(&opp_id).ok_or(())?;
        if opp_index >= seat_opp.slots.len() {
            let seat_me = g.seats.get_mut(&pid).ok_or(())?;
            seat_me.slots[my_index] = Some(my_card);
            return Err(());
        }
        match seat_opp.slots[opp_index].take() {
            Some(c) => c,
            None => {
                let seat_me = g.seats.get_mut(&pid).ok_or(())?;
                seat_me.slots[my_index] = Some(my_card);
                return Err(());
            }
        }
    };

    // Put opponent card into my slot
    {
        let seat_me = g.seats.get_mut(&pid).ok_or(())?;
        seat_me.slots[my_index] = Some(opp_card);
        seat_me.versions[my_index] = seat_me.versions[my_index].wrapping_add(1);
    }
    // Put my card into opponent slot
    {
        let seat_opp = g.seats.get_mut(&opp_id).ok_or(())?;
        seat_opp.slots[opp_index] = Some(my_card);
        seat_opp.versions[opp_index] = seat_opp.versions[opp_index].wrapping_add(1);
    }

    drop(g_lock);
    broadcast_game_update(room);
    end_turn_common(room);
    Ok(())
}

fn handle_power_opp_swap_with_deck(room: &Arc<Room>, pid: Uuid, opp_index: usize) -> Result<(), ()> {
    let mut g_lock = room.game.lock();
    let g = match g_lock.as_mut() { Some(g) => g, None => return Err(()) };
    if g.active != pid { return Err(()) }
    let power_card = match &g.stage { TurnStage::Power { card } => *card, _ => return Err(()) };
    if !(matches!(power_card.rank, Rank::King) && power_card.suit.is_red()) { return Err(()) }
    let opp_id = opponent_of(g, pid).ok_or(())?;
    let old_opt = {
        let seat_opp = g.seats.get_mut(&opp_id).ok_or(())?;
        if opp_index >= seat_opp.slots.len() { return Err(()) }
        seat_opp.slots[opp_index].take()
    };
    if let Some(old) = old_opt {
        let new_opt = draw_from_deck(g);
        {
            let seat_opp = g.seats.get_mut(&opp_id).ok_or(())?;
            if let Some(newc) = new_opt {
                seat_opp.slots[opp_index] = Some(newc);
                seat_opp.versions[opp_index] = seat_opp.versions[opp_index].wrapping_add(1);
                g.deck.push(old);
            } else {
                // restore if deck empty
                seat_opp.slots[opp_index] = Some(old);
            }
        }
    }
    drop(g_lock);
    broadcast_game_update(room);
    end_turn_common(room);
    Ok(())
}

fn handle_end_turn(room: &Arc<Room>, pid: Uuid) -> Result<(), ()> {
    if !is_active(room, pid) { return Err(()) }
    end_turn_common(room);
    Ok(())
}

fn reveal_and_finish(room: &Arc<Room>) {
    let g_opt = room.game.lock().clone();
    let Some(game) = g_opt else { return; };
    let ids: Vec<Uuid> = game.seats.keys().copied().collect();
    if ids.len() != 2 { return; }
    let a = ids[0]; let b = ids[1];
    let seat_a = game.seats.get(&a).unwrap();
    let seat_b = game.seats.get(&b).unwrap();
    let mut score_a = 0u32; let mut score_b = 0u32;
    let mut cards_a = Vec::new(); let mut cards_b = Vec::new();
    for c in seat_a.slots.iter().flatten() { score_a += rank_points(c); cards_a.push(card_public_full(c)); }
    for c in seat_b.slots.iter().flatten() { score_b += rank_points(c); cards_b.push(card_public_full(c)); }
    let winner = if score_a < score_b { Some(a) } else if score_b < score_a { Some(b) } else { None };
    send_to(room, a, &ServerToClient::GameOver { your_score: score_a, opp_score: score_b, you_cards: cards_a.clone(), opp_cards: cards_b.clone(), winner });
    send_to(room, b, &ServerToClient::GameOver { your_score: score_b, opp_score: score_a, you_cards: cards_b, opp_cards: cards_a, winner });
}

// ========== Messages ==========
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerToClient {
    Welcome { player_id: Uuid, lobby: LobbyState },
    LobbyUpdate { lobby: LobbyState },
    GameStart { starting_player: Uuid, mode: GameMode },
    GameUpdate { update: GameUpdate },
    DrawnCard { card: CardPublic },
    PeekResult { target: String, index: usize, card: CardPublic },
    GameOver { your_score: u32, opp_score: u32, you_cards: Vec<CardPublicFull>, opp_cards: Vec<CardPublicFull>, winner: Option<Uuid> },
    Error { message: String },
    Pong,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientToServer {
    Ping,
    Ready,
    DrawDeck,
    DrawDiscard,
    SwapWithHand { index: usize },
    DiscardDrawn,
    MatchTop { index: usize },
    CallZobbo,
    PowerCheckOwn { index: usize },
    PowerCheckOpp { index: usize },
    PowerSwapWithDeck { index: usize },
    PowerSwapWithOpp { my_index: usize, opp_index: usize },
    PowerOppSwapWithDeck { opp_index: usize },
    EndTurn,
}

// ========== Cards and Game State ==========
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Rank { Ace, Two, Three, Four, Five, Six, Seven, Eight, Nine, Ten, Jack, Queen, King }

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Suit { Clubs, Diamonds, Hearts, Spades }

impl Suit { fn is_red(&self) -> bool { matches!(self, Suit::Diamonds | Suit::Hearts) } }

#[derive(Debug, Clone, Copy)]
struct Card { rank: Rank, suit: Suit }

#[derive(Debug, Serialize, Deserialize, Clone)]
struct CardPublic { rank: Rank, is_red_king: bool }

#[derive(Debug, Serialize, Deserialize, Clone)]
struct CardPublicFull { rank: Rank, suit: Suit }

#[derive(Debug, Serialize, Deserialize, Clone)]
struct SlotPublic { empty: bool, version: u64 }

#[derive(Debug, Serialize, Deserialize, Clone)]
struct SeatPublic { slots: Vec<SlotPublic> }

#[derive(Debug, Serialize, Deserialize, Clone)]
struct OpponentPublic { slots: Vec<SlotPublic> }

#[derive(Debug, Serialize, Deserialize, Clone)]
struct GameUpdate {
    you: SeatPublic,
    opponent: OpponentPublic,
    active: Uuid,
    stage: String,
    discard_top: Option<CardPublic>,
    deck_count: u32,
    discard_count: u32,
    zobbo_remaining: Option<u8>,
}

#[derive(Debug, Clone)]
struct Seat { slots: Vec<Option<Card>>, versions: Vec<u64> }

#[derive(Debug, Clone, Copy)]
enum DrawSource { Deck, Discard }

#[derive(Debug, Clone)]
enum TurnStage { AwaitDraw, Holding { card: Card, from: DrawSource }, Power { card: Card } }

#[derive(Debug, Clone)]
struct ZobboState { caller: Uuid, remaining: u8 }

#[derive(Debug, Clone)]
struct GameState {
    deck: Vec<Card>,
    discard: Vec<Card>,
    seats: HashMap<Uuid, Seat>,
    active: Uuid,
    stage: TurnStage,
    skip_next: HashSet<Uuid>,
    zobbo: Option<ZobboState>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct LobbyState {
    room_id: Uuid,
    mode: GameMode,
    players: Vec<LobbyPlayer>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct LobbyPlayer {
    id: Uuid,
    name: String,
    connected: bool,
    ready: bool,
}

impl LobbyState {
    fn from_room(room: &Room) -> Self {
        let players = room
            .players
            .lock()
            .iter()
            .map(|(pid, p)| LobbyPlayer { id: *pid, name: p.name.clone(), connected: p.connected, ready: p.ready })
            .collect();
        Self { room_id: room.id, mode: room.mode.clone(), players }
    }
}

// ========== Lightweight signed token ==========
// token format: base64url(json).base64url(hmac_sha256(json))
fn issue_token(room_id: Uuid, player_id: Uuid) -> anyhow::Result<String> {
    #[derive(Serialize)]
    struct Claims { room: Uuid, player: Uuid, iat: i64 }
    let claims = Claims { room: room_id, player: player_id, iat: OffsetDateTime::now_utc().unix_timestamp() };
    let payload = serde_json::to_vec(&claims)?;
    let part1 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&payload);
    let sig = hmac_sha256(&payload)?;
    let part2 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&sig);
    Ok(format!("{}.{}", part1, part2))
}

fn verify_token(token: &str) -> anyhow::Result<(Uuid, Uuid)> {
    let mut parts = token.split('.');
    let p1 = parts.next().context("missing payload")?;
    let p2 = parts.next().context("missing sig")?;
    if parts.next().is_some() { anyhow::bail!("too many parts") }
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(p1)?;
    let sig = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(p2)?;
    let expected = hmac_sha256(&payload)?;
    if sig != expected { anyhow::bail!("bad signature") }

    #[derive(Deserialize)]
    struct Claims { room: Uuid, player: Uuid, iat: i64 }
    let c: Claims = serde_json::from_slice(&payload)?;
    Ok((c.room, c.player))
}

fn hmac_sha256(data: &[u8]) -> anyhow::Result<[u8; 32]> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;
    let key = HMAC_KEY.get().context("hmac key missing")?;
    let mut mac = HmacSha256::new_from_slice(key).unwrap();
    mac.update(data);
    let out = mac.finalize().into_bytes();
    Ok(out.into())
}

// ========== Static index fallback for any route (for SPA) ==========
async fn index_html() -> Response {
    match fs::read_to_string("../frontend/index.html").await {
        Ok(html) => Html(html).into_response(),
        Err(_) => Html(INDEX_HTML).into_response(),
    }
}

async fn serve_app_js() -> Response {
    match fs::read("../frontend/app.js").await {
        Ok(bytes) => (
            [(header::CONTENT_TYPE, "application/javascript")],
            bytes,
        ).into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "app.js not found").into_response(),
    }
}

async fn serve_main_js() -> Response {
    match fs::read("../frontend/main.js").await {
        Ok(bytes) => (
            [(header::CONTENT_TYPE, "application/javascript")],
            bytes,
        ).into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "main.js not found").into_response(),
    }
}

async fn serve_store_js() -> Response {
    match fs::read("../frontend/store.js").await {
        Ok(bytes) => (
            [(header::CONTENT_TYPE, "application/javascript")],
            bytes,
        ).into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "store.js not found").into_response(),
    }
}

async fn serve_gamestore_js() -> Response {
    match fs::read("../frontend/gameStore.js").await {
        Ok(bytes) => (
            [(header::CONTENT_TYPE, "application/javascript")],
            bytes,
        ).into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "gameStore.js not found").into_response(),
    }
}

async fn serve_component_js(Path(file): Path<String>) -> Response {
    // very basic sanitization
    if file.contains("..") || file.contains('/') || !file.ends_with(".js") {
        return (StatusCode::BAD_REQUEST, "invalid component path").into_response();
    }
    let path = format!("../frontend/components/{}", file);
    match fs::read(path).await {
        Ok(bytes) => (
            [(header::CONTENT_TYPE, "application/javascript")],
            bytes,
        ).into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "component not found").into_response(),
    }
}

async fn serve_favicon() -> Response {
    match fs::read("../frontend/favicon.svg").await {
        Ok(bytes) => (
            [(header::CONTENT_TYPE, "image/svg+xml")],
            bytes,
        ).into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "favicon not found").into_response(),
    }
}

// Embedded minimal index used if static not found (should not be used in normal flow)
const INDEX_HTML: &str = r#"<!doctype html>
<html>
  <head>
    <meta charset=\"utf-8\" />
    <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\" />
    <title>Zobbo</title>
  </head>
  <body>
    <h1>Zobbo server is running</h1>
  </body>
</html>"#;
