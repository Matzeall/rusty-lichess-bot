use std::str::FromStr;

use anyhow::Result;
use shakmaty::{Board, ByRole, Color, uci::UciMove};

pub const QUEEN_VALUE: i32 = 9;
pub const ROOK_VALUE: i32 = 5;
pub const BISHOP_VALUE: i32 = 3;
pub const KNIGHT_VALUE: i32 = 3;
pub const PAWN_VALUE: i32 = 1;

pub fn parse_uci_move(move_str: &str) -> Result<UciMove> {
    let uci_move = UciMove::from_str(move_str.trim())?;

    Ok(uci_move)
}

pub fn parse_uci_moves(move_str: &str) -> Result<Vec<UciMove>> {
    let uci_moves = move_str
        .split_whitespace()
        .map(|s| UciMove::from_str(s.trim()))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(uci_moves)
}

pub fn material_for_side(mat_side: ByRole<u8>) -> i32 {
    let w = mat_side;
    (w.pawn as i32) * PAWN_VALUE
        + (w.knight as i32) * KNIGHT_VALUE
        + (w.bishop as i32) * BISHOP_VALUE
        + (w.rook as i32) * ROOK_VALUE
        + (w.queen as i32) * QUEEN_VALUE
}

pub fn material_difference(position: &Board) -> i32 {
    let white = position.material_side(Color::White);
    let black = position.material_side(Color::Black);
    material_for_side(white) - material_for_side(black)
}
