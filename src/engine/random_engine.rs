use super::Engine;
use anyhow::Result;
use async_trait::async_trait;
use rand::{Rng, rng};
use shakmaty::{Chess, Color, Move, Position, uci::UciMove};

pub struct RandomEngine {
    game: Chess,
    color: Color,
}

impl RandomEngine {
    #[allow(dead_code)]
    pub fn new(initial_position: Chess, bot_color: Color) -> RandomEngine {
        RandomEngine {
            game: initial_position,
            color: bot_color,
        }
    }
}

#[async_trait]
impl Engine for RandomEngine {
    fn is_my_turn(&self) -> bool {
        self.game.turn() == self.color
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
        let legals = self.game.legal_moves();
        if legals.is_empty() {
            return None;
        }
        let rng = rng().random_range(0..legals.len());

        legals.get(rng).cloned()
    }
}
