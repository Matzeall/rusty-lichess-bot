use std::str::FromStr;

use anyhow::Result;
use shakmaty::uci::UciMove;

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
