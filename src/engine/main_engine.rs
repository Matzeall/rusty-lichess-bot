use std::collections::HashMap;

use super::Engine;
use anyhow::Result;
use async_trait::async_trait;
use shakmaty::{Chess, Color, Move, Position, Rank, uci::UciMove};

pub struct MainEngine {
    game: Chess,
    color: Color,
}

impl MainEngine {
    pub fn new(initial_position: Chess, bot_color: Color) -> MainEngine {
        MainEngine {
            game: initial_position,
            color: bot_color,
        }
    }
}

#[async_trait]
impl Engine for MainEngine {
    fn is_my_turn(&self) -> bool {
        !self.game.is_game_over() && self.game.turn() == self.color
    }

    fn get_game_state(&self) -> &Chess {
        &self.game
    }

    async fn update_board(&mut self, move_played: UciMove) -> Result<()> {
        let valid_move = move_played.to_move(&self.game)?;
        self.game.play_unchecked(valid_move);
        Ok(())
    }

    async fn search(&mut self) -> Option<Move> {
        let mut legals = self
            .game
            .legal_moves()
            .into_iter()
            .map(|m| (m, 0))
            .collect::<HashMap<_, _>>();

        if legals.is_empty() {
            return None;
        }

        let strategies: Vec<fn(&Chess, Color, &Move) -> i32> = vec![chaaaaaaarge];
        legals.iter_mut().for_each(|(legal_move, eval)| {
            *eval = strategies
                .iter()
                .map(|strat| strat(&self.game, self.color, legal_move))
                .sum();
        });

        let (chosen_move, _evaluation) = legals.into_iter().max_by_key(|(_, eval)| *eval).unwrap();
        Some(chosen_move)
    }
}

fn chaaaaaaarge(game: &Chess, bot_color: Color, next_move: &Move) -> i32 {
    let mut game = game.clone();
    game.play_unchecked(*next_move);

    let root_rank = if bot_color == Color::White {
        Rank::First
    } else {
        Rank::Eighth
    };

    let eval: u32 = game
        .board()
        .iter()
        .filter(|(_, p)| p.color == bot_color)
        .map(|(sq, _p)| sq.rank().distance(root_rank))
        .sum();

    eval as i32
}
