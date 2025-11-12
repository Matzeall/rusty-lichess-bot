use std::{collections::HashMap, ops::Add};

use crate::util;

use super::Engine;
use anyhow::Result;
use async_trait::async_trait;
use log::{debug, info};
use shakmaty::{Chess, Color, Move, Position, Rank, uci::UciMove};

pub enum Evaluation {
    Additive(i32),
    Absolute(i32),
}
impl Add for Evaluation {
    type Output = Evaluation;

    fn add(self, rhs: Self) -> Self::Output {
        match self {
            Evaluation::Additive(i) => match rhs {
                Evaluation::Additive(j) => Evaluation::Additive(i + j),
                Evaluation::Absolute(j) => Evaluation::Absolute(i.min(j)),
            },
            Evaluation::Absolute(i) => match rhs {
                Evaluation::Additive(_) => self,
                Evaluation::Absolute(j) => Evaluation::Absolute(i.min(j)),
            },
        }
    }
}
impl Evaluation {
    pub fn to_i32(&self) -> i32 {
        match *self {
            Evaluation::Additive(v) => v,
            Evaluation::Absolute(v) => v,
        }
    }
}

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
        let mut legal_moves = self
            .game
            .legal_moves()
            .into_iter()
            .map(|m| (m, 0))
            .collect::<HashMap<_, _>>();
        debug!(
            "{} possible legal moves. Searching for response ...",
            legal_moves.len()
        );

        if legal_moves.is_empty() || self.game.is_game_over() {
            return None;
        }

        let base_eval = self.evaluate_position(&self.game);

        legal_moves.iter_mut().for_each(|(legal_move, eval)| {
            *eval = self.deep_move_evaluation(self.game.clone(), legal_move, 1)
        });

        // sort and get best move
        let mut evaluated_moves = legal_moves.into_iter().collect::<Vec<(_, _)>>();
        evaluated_moves.sort_by_key(|(_, eval)| *eval);
        evaluated_moves.reverse();
        let (chosen_move, best_eval) = *evaluated_moves.first().unwrap();

        // debug
        info!("current eval: {} -> target eval: {}", base_eval, best_eval);
        // evaluated_moves.truncate(3);
        let debug_alternatives = evaluated_moves
            .into_iter()
            .map(|(m, e)| format!("{}  :  {}", m, e))
            .collect::<Vec<_>>()
            .join("\n");
        debug!("Best moves were: \n{}", debug_alternatives);

        Some(chosen_move)
    }
}

impl MainEngine {
    fn deep_move_evaluation(&self, mut game_state: Chess, legal_move: &Move, depth: u8) -> i32 {
        // TODO: variable depth based on early,mid,end-game or strictly by material piece count
        if depth > 3 {
            return self.evaluate_position(&game_state);
        }

        game_state.play_unchecked(*legal_move);
        let legal_moves = game_state.legal_moves();
        // TODO: apply pruning techniques to save counteract the exponential growth

        let is_bots_turn = game_state.turn() == self.color;

        // this simultaneously handles checkmate rewards
        let mut deeper_eval = if is_bots_turn { i32::MIN } else { i32::MAX };
        for m in legal_moves {
            let eval = self.deep_move_evaluation(game_state.clone(), &m, depth + 1);
            if is_bots_turn {
                deeper_eval = deeper_eval.max(eval); // maximize evaluation
            } else {
                deeper_eval = deeper_eval.min(eval); // assume opponent wants to win too
            }
        }

        deeper_eval
    }

    fn evaluate_position(&self, game_state: &Chess) -> i32 {
        // TODO: need performance metrics per strategy and overall
        let strategies: Vec<fn(&Chess, Color) -> Evaluation> =
            vec![material_difference, evaluate_draw];

        let mut eval_summed = Evaluation::Additive(0);
        for strategy in strategies {
            eval_summed = eval_summed + strategy(game_state, self.color);
        }
        eval_summed.to_i32()
    }
}

//////////////////////////  STRATEGIES  /////////////////////////////////////////

/// Main "Tactics" strategy
fn material_difference(game: &Chess, bot_color: Color) -> Evaluation {
    let side = if bot_color == Color::White { 1 } else { -1 };
    Evaluation::Additive(util::material_difference(game.board()) * side)
}

// overwrite any strategy on draw to a 0 - Evaluation
fn evaluate_draw(game: &Chess, _bot_color: Color) -> Evaluation {
    match game.is_stalemate() || game.is_insufficient_material() {
        true => Evaluation::Absolute(0),
        false => Evaluation::Additive(0),
    }
}

/// funny
#[allow(dead_code)]
fn chaaaaaaarge(game: &Chess, bot_color: Color) -> Evaluation {
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

    Evaluation::Additive(eval as i32)
}
