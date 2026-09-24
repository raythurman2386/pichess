# Puzzle pack

`starter.jsonl` is a filtered slice of the [Lichess puzzle database](https://database.lichess.org/#puzzles), which Lichess releases under [CC0](https://creativecommons.org/publicdomain/zero/1.0/). Each line is one puzzle: the FEN is the position before the opponent's setup move, and `moves` is the UCI line starting with that setup move.

Popularity is Lichess's vote score (100 is best). This pack keeps puzzles players rated well, spread across ratings, so the queue is tactics from real games rather than generated positions.
