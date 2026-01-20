//! Typed redraw events from Neovim
//!
//! Inspired by neovide's event parsing, this module provides type-safe
//! parsing of Neovim redraw events.

use rmpv::Value;

/// Redraw events from Neovim UI protocol
#[derive(Debug, Clone, PartialEq)]
pub enum RedrawEvent {
    /// Mode changed (mode_name, mode_index)
    ModeChange { mode: String, mode_index: u64 },
    /// Cursor moved to position on grid
    GridCursorGoto { grid: u64, row: u64, col: u64 },
    /// Window viewport changed (from ext_multigrid)
    /// Contains viewport information for scroll synchronization
    WinViewport {
        grid: u64,
        win: i64,
        topline: i64,
        botline: i64,
        curline: i64,
        curcol: i64,
        line_count: i64,
        scroll_delta: i64,
    },
    /// Flush signals end of redraw batch
    Flush,
    /// Unknown or unhandled event
    Unknown(String),
}

/// Error type for event parsing
#[derive(Debug, Clone)]
pub struct ParseError {
    pub event_name: String,
    pub reason: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Failed to parse '{}': {}", self.event_name, self.reason)
    }
}

impl std::error::Error for ParseError {}

impl RedrawEvent {
    /// Parse a single redraw event from msgpack Value
    pub fn parse(event_data: &[Value]) -> Result<Vec<RedrawEvent>, ParseError> {
        let event_name = event_data
            .first()
            .and_then(|v| v.as_str())
            .ok_or_else(|| ParseError {
                event_name: "unknown".to_string(),
                reason: "Missing event name".to_string(),
            })?;

        let mut events = Vec::new();

        match event_name {
            "mode_change" => {
                // mode_change: ["mode_change", [mode_name, mode_idx], ...]
                for i in 1..event_data.len() {
                    if let Some(event) = Self::parse_mode_change(event_data.get(i))? {
                        events.push(event);
                    }
                }
            }
            "grid_cursor_goto" => {
                // grid_cursor_goto: ["grid_cursor_goto", [grid, row, col], ...]
                for i in 1..event_data.len() {
                    if let Some(event) = Self::parse_grid_cursor_goto(event_data.get(i))? {
                        events.push(event);
                    }
                }
            }
            "win_viewport" => {
                // win_viewport: ["win_viewport", [grid, win, topline, botline, curline, curcol, line_count, scroll_delta], ...]
                for i in 1..event_data.len() {
                    if let Some(event) = Self::parse_win_viewport(event_data.get(i))? {
                        events.push(event);
                    }
                }
            }
            "flush" => {
                events.push(RedrawEvent::Flush);
            }
            _ => {
                // Unknown event - store for debugging if needed
                events.push(RedrawEvent::Unknown(event_name.to_string()));
            }
        }

        Ok(events)
    }

    fn parse_mode_change(value: Option<&Value>) -> Result<Option<RedrawEvent>, ParseError> {
        let Some(Value::Array(mode_info)) = value else {
            return Ok(None);
        };

        let mode = mode_info
            .first()
            .and_then(|v| v.as_str())
            .ok_or_else(|| ParseError {
                event_name: "mode_change".to_string(),
                reason: "Missing mode name".to_string(),
            })?
            .to_string();

        let mode_index = mode_info.get(1).and_then(|v| v.as_u64()).unwrap_or(0);

        Ok(Some(RedrawEvent::ModeChange { mode, mode_index }))
    }

    fn parse_grid_cursor_goto(value: Option<&Value>) -> Result<Option<RedrawEvent>, ParseError> {
        let Some(Value::Array(cursor_info)) = value else {
            return Ok(None);
        };

        if cursor_info.len() < 3 {
            return Err(ParseError {
                event_name: "grid_cursor_goto".to_string(),
                reason: format!("Expected 3 values, got {}", cursor_info.len()),
            });
        }

        let grid = cursor_info
            .first()
            .and_then(|v| v.as_u64())
            .ok_or_else(|| ParseError {
                event_name: "grid_cursor_goto".to_string(),
                reason: "Invalid grid id".to_string(),
            })?;

        let row = cursor_info
            .get(1)
            .and_then(|v| v.as_u64())
            .ok_or_else(|| ParseError {
                event_name: "grid_cursor_goto".to_string(),
                reason: "Invalid row".to_string(),
            })?;

        let col = cursor_info
            .get(2)
            .and_then(|v| v.as_u64())
            .ok_or_else(|| ParseError {
                event_name: "grid_cursor_goto".to_string(),
                reason: "Invalid col".to_string(),
            })?;

        Ok(Some(RedrawEvent::GridCursorGoto { grid, row, col }))
    }

    fn parse_win_viewport(value: Option<&Value>) -> Result<Option<RedrawEvent>, ParseError> {
        let Some(Value::Array(info)) = value else {
            return Ok(None);
        };

        if info.len() < 8 {
            return Err(ParseError {
                event_name: "win_viewport".to_string(),
                reason: format!("Expected 8 values, got {}", info.len()),
            });
        }

        // Fields: grid, win, topline, botline, curline, curcol, line_count, scroll_delta
        let grid = info
            .first()
            .and_then(|v| v.as_u64())
            .ok_or_else(|| ParseError {
                event_name: "win_viewport".to_string(),
                reason: "Invalid grid id".to_string(),
            })?;

        let win = info.get(1).and_then(|v| v.as_i64()).unwrap_or(0);
        let topline = info.get(2).and_then(|v| v.as_i64()).unwrap_or(0);
        let botline = info.get(3).and_then(|v| v.as_i64()).unwrap_or(0);
        let curline = info.get(4).and_then(|v| v.as_i64()).unwrap_or(0);
        let curcol = info.get(5).and_then(|v| v.as_i64()).unwrap_or(0);
        let line_count = info.get(6).and_then(|v| v.as_i64()).unwrap_or(0);
        let scroll_delta = info.get(7).and_then(|v| v.as_i64()).unwrap_or(0);

        Ok(Some(RedrawEvent::WinViewport {
            grid,
            win,
            topline,
            botline,
            curline,
            curcol,
            line_count,
            scroll_delta,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mode_change() {
        let event_data = vec![
            Value::from("mode_change"),
            Value::Array(vec![Value::from("i"), Value::from(1u64)]),
        ];

        let events = RedrawEvent::parse(&event_data).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            RedrawEvent::ModeChange {
                mode: "i".to_string(),
                mode_index: 1
            }
        );
    }

    #[test]
    fn test_parse_grid_cursor_goto() {
        let event_data = vec![
            Value::from("grid_cursor_goto"),
            Value::Array(vec![
                Value::from(1u64),
                Value::from(10u64),
                Value::from(5u64),
            ]),
        ];

        let events = RedrawEvent::parse(&event_data).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            RedrawEvent::GridCursorGoto {
                grid: 1,
                row: 10,
                col: 5
            }
        );
    }

    #[test]
    fn test_parse_win_viewport() {
        let event_data = vec![
            Value::from("win_viewport"),
            Value::Array(vec![
                Value::from(1u64),    // grid
                Value::from(1000i64), // win
                Value::from(10i64),   // topline
                Value::from(30i64),   // botline
                Value::from(15i64),   // curline
                Value::from(5i64),    // curcol
                Value::from(100i64),  // line_count
                Value::from(0i64),    // scroll_delta
            ]),
        ];

        let events = RedrawEvent::parse(&event_data).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            RedrawEvent::WinViewport {
                grid: 1,
                win: 1000,
                topline: 10,
                botline: 30,
                curline: 15,
                curcol: 5,
                line_count: 100,
                scroll_delta: 0,
            }
        );
    }

    #[test]
    fn test_parse_flush() {
        let event_data = vec![Value::from("flush")];

        let events = RedrawEvent::parse(&event_data).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], RedrawEvent::Flush);
    }

    #[test]
    fn test_parse_unknown() {
        let event_data = vec![Value::from("some_unknown_event")];

        let events = RedrawEvent::parse(&event_data).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            RedrawEvent::Unknown("some_unknown_event".to_string())
        );
    }
}
