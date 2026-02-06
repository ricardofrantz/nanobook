use crate::event::PyEvent;
use nanobook::itch::{ItchParser, itch_to_event};
use pyo3::prelude::*;
use std::fs::File;
use std::io::BufReader;

#[pyfunction]
pub fn parse_itch(path: &str) -> PyResult<Vec<(String, PyEvent)>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut parser = ItchParser::new(reader);

    let mut events = Vec::new();

    while let Some(msg) = parser
        .next_message()
        .map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string()))?
    {
        if let Some((symbol, event)) = itch_to_event(msg) {
            events.push((symbol, PyEvent { inner: event }));
        }
    }

    Ok(events)
}
