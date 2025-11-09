mod engine;
mod util;

use crate::engine::Engine;
use anyhow::{Result, bail};
use env_logger::{Env, Target};
use futures::StreamExt;
use licheszter::{
    client::Licheszter,
    models::{
        board::{BoardState, Event},
        challenge::ChallengeStatus,
        chat::ChatRoom,
        game::{GameEventInfo, GameStatus, VariantMode},
    },
};
use log::{debug, error, info};
use shakmaty::{CastlingMode, Chess, Color, Position, Square, fen::Fen};
use std::{env, str::FromStr, sync::Arc};
use util::{parse_uci_move, parse_uci_moves};

const MAX_SIMULTANEOUS_GAMES: usize = 3;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // start with "./rusty_lichess_bot 2>&1 | tee -a /path/to/rusty_lichess_bot.log" for a log-file
    env_logger::Builder::from_env(Env::default().default_filter_or("info"))
        .target(Target::Stdout)
        .init();

    // Read the Lichess BOT token from env or local file
    let token = match env::var("LICHESS_BOT_TOKEN") {
        Ok(t) => t,
        Err(_) => {
            let path = std::env::current_dir()?.join("LICHESS_BOT_TOKEN");
            std::fs::read_to_string(path)?.trim().to_string()
        }
    };

    let client = Arc::new(Licheszter::builder().with_authentication(token).build());

    info!("Bot connected - listening for events...");

    let mut waiting_challenges = Vec::new();
    let mut events = client.connect().await.unwrap();
    while let Some(possible_event) = events.next().await {
        match possible_event {
            Ok(event) => match event {
                Event::Challenge { challenge } => {
                    if challenge.status != ChallengeStatus::Created {
                        continue;
                    }

                    let user = challenge.challenger;
                    info!(
                        "[{}] Challenge recieved.\n   Time control: {:?}.\n    Challenger: {} (rating: {:?})",
                        challenge.id, challenge.time_control, user.name, user.rating
                    );
                    // fetch active games by bot to decide whether to take on the next opponent
                    let active_games = client.games_ongoing(50).await?;
                    if active_games.len() < MAX_SIMULTANEOUS_GAMES {
                        client
                            .challenge_accept(&challenge.id)
                            .await
                            .expect("Error when accepting challenge.");
                    } else {
                        waiting_challenges.insert(0, challenge.id);
                    }
                }
                Event::GameStart { game: game_info } => {
                    let game_id = game_info.id.clone();
                    info!(
                        "[{}] New Game against {}",
                        game_id, game_info.opponent.username
                    );

                    tokio::spawn(spawn_engine(client.clone(), game_info));
                }
                Event::GameFinish { game } => {
                    info!("[{}] GameEnd", game.id);

                    // check if there are any other waiting challengers
                    if let Some(ch_id) = waiting_challenges.pop() {
                        client
                            .challenge_accept(&ch_id)
                            .await
                            .expect("Error when accepting challenge.");
                    }
                }
                Event::ChallengeCanceled { challenge: ch } => {
                    info!("received lichess event: ChallengeCanceled ({})", &ch.id);
                    waiting_challenges.retain(|c| c != &ch.id);
                }
                Event::ChallengeDeclined { challenge: _ } => {
                    info!("received lichess event: ChallengeDeclined")
                }
            },
            Err(e) => error!("{}", e),
        }
        // Do something with the event!
    }

    Ok(())
}

async fn spawn_engine(client: Arc<Licheszter>, game_id: GameEventInfo) {
    match spawn_engine_internal(client, game_id).await {
        Ok(()) => info!("engine finished! exiting & dropping engine instance ..."),
        Err(e) => error!("engine failed because, {e}"),
    };
}

async fn spawn_engine_internal(client: Arc<Licheszter>, game_id: GameEventInfo) -> Result<()> {
    let mut engine: Option<Box<dyn Engine>> = None;
    let mut stream = client
        .bot_game_connect(&game_id.id)
        .await
        .expect("Error while feathing game state stream.");

    debug!("got stream handle for game {}", game_id.id);
    // stream event loop
    while let Some(item) = stream.next().await {
        match item {
            Ok(state) => {
                match state {
                    BoardState::GameFull(game_full) => {
                        debug!("Game Full Event");

                        let mut game = shakmaty::Chess::new(); // default pos
                        // in most game modes the initial_fen is no FEN, but "startpos"
                        if &game_full.initial_fen != "startpos" {
                            info!("Inital FEN: {}", &game_full.initial_fen);
                            let initial_pos = Fen::from_str(&game_full.initial_fen)
                                .expect("GameStart Event delivered invalid inital position FEN.");

                            let castling_mode = match game_full.variant.key {
                                VariantMode::Chess960 => CastlingMode::Chess960,
                                _ => CastlingMode::Standard,
                            };

                            game = initial_pos
                                .into_position(castling_mode)
                                .expect("GameStart Event delivered invalid initial position.");
                        }

                        let bot_color = match game_full.white.name == game_id.opponent.username {
                            true => Color::Black,
                            false => Color::White,
                        };

                        // setup engine with the default board of the current game mode
                        engine = Some(engine::init_engine(game, bot_color));

                        match &mut engine {
                            Some(engine) => {
                                info!("[{}] Game started. Bot plays {:?}.", game_id.id, bot_color);

                                if !game_full.state.moves.is_empty() {
                                    info!(
                                        "[{}] Game was already in progress. Moves already played: {}, catching up...",
                                        game_id.id, game_full.state.moves
                                    );
                                    // try updating initial chess board to match already played moves
                                    let uci_moves = parse_uci_moves(&game_full.state.moves)?;
                                    for uci_move in uci_moves {
                                        engine.update_board(uci_move).await?
                                    }
                                }

                                if engine.is_my_turn() {
                                    bot_play_move(&client, &game_id, engine).await?;
                                }
                            }
                            None => {
                                abort_game_cleanly_after_error(
                                    &client,
                                    &game_id,
                                    "Engine was not in a valid state after initialization",
                                    Some("I could not setup myself correctly. I will resign now"),
                                )
                                .await? // will return error
                            }
                        }
                    }
                    BoardState::GameState(game_state) => {
                        match game_state.status {
                            GameStatus::Started => {
                                match &mut engine {
                                    Some(engine) => {
                                        // there seems to be no better way to get the last move
                                        let last_move = game_state
                                            .moves.rsplit(" ")
                                            .next()
                                            .expect("Move string should contain a substring when splitting by space.");

                                        log_move(last_move, engine.get_game_state(), &game_id.id)
                                            .await?;

                                        // update position to current
                                        let uci_move = parse_uci_move(last_move)?;
                                        engine.update_board(uci_move).await?;

                                        if engine.is_my_turn() {
                                            bot_play_move(&client, &game_id, engine).await?;
                                        }
                                    }
                                    None => {
                                        abort_game_cleanly_after_error(
                                            &client,
                                            &game_id,
                                            "engine not valid",
                                            None,
                                        )
                                        .await?
                                    }
                                }
                            }
                            status => {
                                info!("received game status {:?}", status)
                            }
                        }
                    }
                    game_state => {
                        info!(
                            "[{}] Other board state recieved: {:?}",
                            game_id.id, game_state
                        );
                    }
                }
            }
            Err(e) => {
                error!("Error from game stream: {:?}", e);
            }
        };
    }

    Ok(())
}

async fn log_move(last_move: &str, in_game_state: &Chess, for_game: &str) -> Result<()> {
    let origin_str = &last_move[0..2];
    let dest_str = &last_move[2..4];

    let from = Square::from_str(origin_str)?;
    let to = Square::from_str(dest_str)?;
    let moved_piece = in_game_state
        .board()
        .piece_at(from)
        .expect("at last uci move's origin square is no piece that could be moved??");
    let taken_piece = in_game_state.board().piece_at(to);

    info!(
        "[{}]  {:>2}. {} played {:?} ({}) to {}{}",
        for_game,
        in_game_state.fullmoves(),
        in_game_state.turn().to_string().to_uppercase(),
        moved_piece.role,
        from,
        to,
        taken_piece.map_or("".to_string(), |p| format!(", taking {:?}", p.role))
    );

    Ok(())
}

async fn bot_play_move(
    client: &Arc<Licheszter>,
    game_id: &GameEventInfo,
    engine: &mut Box<dyn Engine>,
) -> Result<(), anyhow::Error> {
    if let Some(chosen_move) = engine.search().await {
        // convert move back to uci and send to lichess.org
        let uci_move = chosen_move.to_uci(CastlingMode::Standard).to_string();
        client
            .bot_play_move(&game_id.id, &uci_move, false)
            .await
            .unwrap_or_else(|_| panic!("Error when making move ({uci_move})"));
    } else {
        abort_game_cleanly_after_error(
            client,
            game_id,
            "Engine could not compute a move to play!",
            Some("I couldn't find a move to play. I will resign now"),
        )
        .await?
    }

    Ok(())
}

async fn abort_game_cleanly_after_error(
    client: &Arc<Licheszter>,
    game_id: &GameEventInfo,
    error: &str,
    chat_message: Option<&str>,
) -> Result<(), anyhow::Error> {
    error!("{}\nFEN: {}", error, game_id.fen);

    if let Some(msg) = chat_message {
        client
            .bot_chat_write(&game_id.game_id, ChatRoom::Spectator, msg)
            .await?;
    }
    client
        .bot_chat_write(&game_id.game_id, ChatRoom::Spectator, &game_id.fen)
        .await?;

    client.bot_game_resign(&game_id.game_id).await?;

    bail!("{}", error)
}
