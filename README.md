# Zobbo

A fun online competitive card game to play with your friends.
Mainly just making this to play with my S/O so we can play when we're long distance again </3

Aim of the Game:
Have the lowest total points score left in your card roster until someone calls Zobbo

How to Win?
Before you take your next card on your turn: Call Zobbo when you think you have less points than your opponent
Finish your turn
Opponent finishes their turn 
Flip all cards and count the cards

Zobbo Rules :
6 Cards each (1v1)
You start with 6 cards (downfacing) each from the shuffled deck,
You can order them how you like without looking
Then check the bottom 3 cards

Start the game 

On each turn:
```py
if take_deck_card():
  if swap_with_roster_card():
    end_turn()
  elif discard():
    use_card_power()
    end_turn()
elif take_discard_pile_card():
  swap_with_roster_card()
  end_turn()
```
Anytime:
You can match a card in the discarded card pile with your own card, leaving you with less cards in total

Card Powers: (Only counts if picked up from deck)
5,6,7,8 - Check own card 
9,10 - Check other players card
J - Swap your card with next card on the deck
Q - Swap cards with player without looking
Red K - Swap opponents card with next card on the deck
Black K - Worth 0 points

Note: You can choose to either use the power or discard the card or put it in you card roster

Card Values (Lowest to Highest):
Black King: 0
Ace: 1
Number Value:
2,3,4,5,6,7,8,9,10
Jack: 11
Queen: 12
Red King: 13

Two Game Modes:
Sudden Death:
Play one round of zobbo and only who wins counts

Zobbo Battle:
Choose how many rounds of zobbo you would like to play,
Play each round,
Your total points are added at the end of each round,
If you win no points are added,
If you lose your points are added
At the end of the battle whoever got the lowest points wins

Profile Stats: 
Total number of wins will be tracked
Total points will be tracked (Same rules as Zobbo Battle)
  Monthly, Of all Time 
