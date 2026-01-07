use std::{
    cmp::min_by_key,
    collections::HashMap,
    ops::Add,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::util;

use super::Engine;
use anyhow::Result;
use async_trait::async_trait;
use log::{debug, info};
use shakmaty::{Chess, Color, Move, Position, Rank, uci::UciMove};

const MAX_EVAL: i32 = 1_000_000;
const MIN_EVAL: i32 = -1_000_000;

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
                Evaluation::Absolute(j) => Evaluation::Absolute(j),
            },
            Evaluation::Absolute(i) => match rhs {
                Evaluation::Additive(_) => self,
                // choose least extreme evaluation
                Evaluation::Absolute(j) => Evaluation::Absolute(min_by_key(i, j, |num| num.abs())),
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

struct StatsSubsystem {
    current_target_eval: i32,
    pruning_cutoffs: Vec<u32>,
}
impl StatsSubsystem {
    fn new() -> Self {
        Self {
            current_target_eval: 0,
            pruning_cutoffs: Vec::new(),
        }
    }
    fn reset_move_metrics(&mut self, search_depth: u8) {
        self.pruning_cutoffs = vec![0u32; search_depth as usize];
    }
}

pub struct MainEngine {
    game: Chess,
    color: Color,
    stats: StatsSubsystem,
}
impl MainEngine {
    pub fn new(initial_position: Chess, bot_color: Color) -> MainEngine {
        MainEngine {
            game: initial_position,
            color: bot_color,
            stats: StatsSubsystem::new(),
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
        let start_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();

        let mut legal_moves = self
            .game
            .legal_moves()
            .into_iter()
            .map(|m| (m, 0))
            .collect::<HashMap<_, _>>();

        if legal_moves.is_empty() || self.game.is_game_over() {
            return None;
        }

        info!(
            "Searching for response. {} possible legal moves available",
            legal_moves.len()
        );

        // TODO: dynamically adjust search_depth based on time_remaining?
        // TODO: variable depth based on early,mid,end-game or strictly by material/piece count
        let search_depth = 4;
        self.stats.reset_move_metrics(search_depth);

        // pass updated alpha (best eval bot can force against sensible enemy, which was seen before) to next children
        let mut alpha = MIN_EVAL - 1000; // make sure it's still smaller than checkmate
        legal_moves.iter_mut().for_each(|(legal_move, eval)| {
            *eval = self.deep_move_evaluation(
                self.game.clone(),
                legal_move,
                search_depth - 1,
                alpha,
                MAX_EVAL,
            );
            alpha = alpha.max(*eval);
        });

        // sort and get best move
        let mut evaluated_moves = legal_moves.into_iter().collect::<Vec<(_, _)>>();
        evaluated_moves.sort_by_key(|(_, eval)| *eval);
        evaluated_moves.reverse();
        let (chosen_move, best_eval) = *evaluated_moves.first().unwrap();

        // log stats and debug info
        // TODO: improve stats subsystem to show actual lines to make debugging easier
        let execution_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap() - start_time;
        info!(
            "Chose {chosen_move} (eval: {} -> {best_eval}, searched: {:.2}s, alpha-beta cutoffs: | {} | )",
            self.stats.current_target_eval,
            execution_time.as_secs_f32(),
            self.stats
                .pruning_cutoffs
                .iter()
                .rev()
                .enumerate()
                .map(|(i, n)| { format!("{i} : {n}") })
                .collect::<Vec<_>>()
                .join(" | ")
        );
        debug!(
            "Calculated lines were: \n{}",
            evaluated_moves
                .into_iter()
                .map(|(m, e)| format!("{:>6}  :  {:+}", m.to_string(), e))
                .collect::<Vec<_>>()
                .join("\n")
        );

        self.stats.current_target_eval = best_eval;

        Some(chosen_move)
    }
}

impl MainEngine {
    /// adaptation of the minimax algorithm with alpha-beta pruning
    fn deep_move_evaluation(
        &mut self,
        mut game_state: Chess,
        legal_move: &Move,
        depth: u8,
        mut alpha: i32, // highest eval the bot can force, assuming best play from opponent
        mut beta: i32,  // smallest eval the opponent can force, assuming best play from bot
    ) -> i32 {
        game_state.play_unchecked(*legal_move);

        if depth == 0 || game_state.is_game_over() {
            return self.evaluate_position(&game_state);
        }

        let legal_moves = game_state.legal_moves();
        assert!(!legal_moves.is_empty()); // terminated games should have been catched earlier

        // TODO: sort moves by likelyhood of being good to get the most out of pruning

        let is_bots_turn = game_state.turn() == self.color;
        let mut deeper_eval = if is_bots_turn { MIN_EVAL } else { MAX_EVAL };

        for m in legal_moves {
            let eval = self.deep_move_evaluation(game_state.clone(), &m, depth - 1, alpha, beta);
            if is_bots_turn {
                deeper_eval = deeper_eval.max(eval); // maximize bots evaluation on his turn
                alpha = alpha.max(eval);
                // TODO: > vs >= (bot only plays well with > and <, I don't fully understand why,
                // conceptually pruning already makes sense for >=) -> investigate further
                if eval > beta {
                    self.stats.pruning_cutoffs[depth as usize - 1] += 1;
                    break;
                }
            } else {
                deeper_eval = deeper_eval.min(eval); // assume opponent wants to win too
                beta = beta.min(eval);
                if eval < alpha {
                    self.stats.pruning_cutoffs[depth as usize - 1] += 1;
                    break;
                }
            }
        }

        deeper_eval
    }

    /// the actual evaluation function, which combines the expected positional value of each
    /// applied strategy/tactic-function (Sum operator of Evaluation is adjusted)
    /// Most strategy functions should only nudge the Evaluation a tiny bit compared to the
    /// material_difference strategy(Pawn-win = +100), so they apply only in case of not having
    /// the oportunity to win material directly. Exceptions: Checkmate and Stalemate strategies.
    fn evaluate_position(&mut self, game_state: &Chess) -> i32 {
        // TODO: need performance metrics per strategy and overall
        let strategies: Vec<fn(&Chess, Color) -> Evaluation> =
            vec![material_difference, evaluate_checkmate, evaluate_draw];

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
        false => Evaluation::Additive(0), // no-op
    }
}

fn evaluate_checkmate(game: &Chess, bot_color: Color) -> Evaluation {
    match game.is_checkmate() {
        true => Evaluation::Absolute(if game.turn() != bot_color {
            MAX_EVAL - game.fullmoves().get() as i32 // prefers faster checkmates
        } else {
            MIN_EVAL + game.fullmoves().get() as i32 // prefers faster checkmates
        }),
        false => Evaluation::Additive(0), // no-op
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
