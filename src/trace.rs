//! TRES-trace witnesses for the VoxBox Station.
//!
//! Implements the frozen TraceEvent layout (32 bytes, align 8), the 21
//! canonical verb indices, and the §7 status codes. Every state transition
//! the station performs emits a witness; traces may be reordered, never
//! removed (Edge invariant 2).
use anyhow::{Context, Result};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Verb {
    Plot = 0,
    Frame = 1,
    Invite = 2,
    Weave = 3,
    Tend = 4,
    Display = 5,
    Sprout = 6,
    Pot = 7,
    Graft = 8,
    Produce = 9,
    Bloom = 10,
    Crop = 11,
    Trace = 12,
    Yield = 13,
    Dormant = 14,
    Recycle = 15,
    Cut = 16,
    Compost = 17,
    Seek = 18,
    Survey = 19,
    Sense = 20,
}

impl Verb {
    pub fn index(self) -> u8 {
        self as u8
    }

    pub fn chapter(self) -> u8 {
        match self.index() {
            0..=5 => 0,
            6..=17 => 1,
            _ => 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Verb::Plot => "plot",
            Verb::Frame => "frame",
            Verb::Invite => "invite",
            Verb::Weave => "weave",
            Verb::Tend => "tend",
            Verb::Display => "display",
            Verb::Sprout => "sprout",
            Verb::Pot => "pot",
            Verb::Graft => "graft",
            Verb::Produce => "produce",
            Verb::Bloom => "bloom",
            Verb::Crop => "crop",
            Verb::Trace => "trace",
            Verb::Yield => "yield",
            Verb::Dormant => "dormant",
            Verb::Recycle => "recycle",
            Verb::Cut => "cut",
            Verb::Compost => "compost",
            Verb::Seek => "seek",
            Verb::Survey => "survey",
            Verb::Sense => "sense",
        }
    }

    pub fn from_index(i: u8) -> Option<Verb> {
        Some(match i {
            0 => Verb::Plot,
            1 => Verb::Frame,
            2 => Verb::Invite,
            3 => Verb::Weave,
            4 => Verb::Tend,
            5 => Verb::Display,
            6 => Verb::Sprout,
            7 => Verb::Pot,
            8 => Verb::Graft,
            9 => Verb::Produce,
            10 => Verb::Bloom,
            11 => Verb::Crop,
            12 => Verb::Trace,
            13 => Verb::Yield,
            14 => Verb::Dormant,
            15 => Verb::Recycle,
            16 => Verb::Cut,
            17 => Verb::Compost,
            18 => Verb::Seek,
            19 => Verb::Survey,
            20 => Verb::Sense,
            _ => return None,
        })
    }
}

pub const OK: i32 = 0;
pub const E_NOT_FOUND: i32 = -1;
pub const E_NOT_READY: i32 = -2;
pub const E_ALREADY_SPROUTED: i32 = -3;
pub const E_INVALID: i32 = -4;
pub const E_WOULD_BLOCK: i32 = -5;
pub const E_PERMISSION: i32 = -6;
pub const E_VERSION_MISMATCH: i32 = -7;
pub const E_HARDWARE_MISMATCH: i32 = -8;
pub const E_CORRUPT: i32 = -9;
pub const E_OVERFLOW: i32 = -10;
pub const E_TIMEOUT: i32 = -11;
pub const E_FAULT: i32 = -12;
pub const E_FALLOW: i32 = -13;
pub const E_NOT_IMPLEMENTED: i32 = -14;
pub const E_RESOURCE: i32 = -15;
pub const E_STATE: i32 = -16;
pub const E_UTF8: i32 = -17;

/// TRES_ABI §3.6 TraceEvent. 32 bytes, align 8.
#[repr(C, align(8))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceEvent {
    pub verb: u8,
    pub chapter: u8,
    pub pad: [u8; 2],
    pub outcome: i32,
    pub target: u64,
    pub timestamp: u64,
    pub provenance: u64,
}

impl TraceEvent {
    pub const SIZE: usize = 32;

    pub fn new(verb: Verb, outcome: i32, target: u64, provenance: u64) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        TraceEvent {
            verb: verb.index(),
            chapter: verb.chapter(),
            pad: [0, 0],
            outcome,
            target,
            timestamp,
            provenance,
        }
    }

    pub fn to_bytes(&self) -> [u8; 32] {
        let mut b = [0u8; 32];
        b[0] = self.verb;
        b[1] = self.chapter;
        b[4..8].copy_from_slice(&self.outcome.to_le_bytes());
        b[8..16].copy_from_slice(&self.target.to_le_bytes());
        b[16..24].copy_from_slice(&self.timestamp.to_le_bytes());
        b[24..32].copy_from_slice(&self.provenance.to_le_bytes());
        b
    }

    pub fn from_bytes(b: &[u8]) -> Option<TraceEvent> {
        if b.len() < 32 {
            return None;
        }
        Some(TraceEvent {
            verb: b[0],
            chapter: b[1],
            pad: [0, 0],
            outcome: i32::from_le_bytes(b[4..8].try_into().ok()?),
            target: u64::from_le_bytes(b[8..16].try_into().ok()?),
            timestamp: u64::from_le_bytes(b[16..24].try_into().ok()?),
            provenance: u64::from_le_bytes(b[24..32].try_into().ok()?),
        })
    }

    pub fn verb_name(&self) -> Option<&'static str> {
        Verb::from_index(self.verb).map(|v| v.name())
    }

    /// The non-empty witness line: verb, outcome, ids, nanos.
    pub fn witness(&self) -> String {
        let verb = self.verb_name().unwrap_or("reserved");
        format!(
            "{} chapter={} outcome={} target={:#x} nanos={} provenance={:#x}",
            verb, self.chapter, self.outcome, self.target, self.timestamp, self.provenance
        )
    }
}

/// Append-only witness log: binary 32-byte records + human-readable witness
/// lines. The trace may be reordered, never removed.
pub struct TraceLog {
    path: PathBuf,
    last: u64,
}

impl TraceLog {
    pub fn open(path: PathBuf) -> Result<TraceLog> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(TraceLog { path, last: 0 })
    }

    pub fn default_path() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("voxbox/trace.trace")
    }

    /// Emit one event: appends the binary record and its non-empty witness.
    /// Returns the witness line. Provenance chains to the previous event.
    pub fn emit(&mut self, event: TraceEvent) -> Result<String> {
        let event = TraceEvent { provenance: self.last, ..event };
        let witness = event.witness();
        self.last = event.timestamp;
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .with_context(|| format!("opening trace log {}", self.path.display()))?;
        f.write_all(&event.to_bytes())?;
        writeln!(f, "{}", witness)?;
        Ok(witness)
    }

    pub fn emit_ok(&mut self, verb: Verb, target: u64) -> Result<String> {
        self.emit(TraceEvent::new(verb, OK, target, 0))
    }

    pub fn emit_fail(&mut self, verb: Verb, outcome: i32, target: u64) -> Result<String> {
        self.emit(TraceEvent::new(verb, outcome, target, 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_event_is_32_bytes_align_8() {
        assert_eq!(std::mem::size_of::<TraceEvent>(), 32);
        assert_eq!(std::mem::align_of::<TraceEvent>(), 8);
        assert_eq!(TraceEvent::SIZE, 32);
    }

    #[test]
    fn verb_indices_match_tres_abi() {
        assert_eq!(Verb::Plot.index(), 0);
        assert_eq!(Verb::Display.index(), 5);
        assert_eq!(Verb::Sprout.index(), 6);
        assert_eq!(Verb::Trace.index(), 12);
        assert_eq!(Verb::Yield.index(), 13);
        assert_eq!(Verb::Compost.index(), 17);
        assert_eq!(Verb::Sense.index(), 20);
        for i in 0u8..21 {
            assert_eq!(Verb::from_index(i).map(|v| v.index()), Some(i));
        }
        assert_eq!(Verb::from_index(21), None);
    }

    #[test]
    fn chapters_split_6_12_3() {
        assert_eq!(Verb::Plot.chapter(), 0);
        assert_eq!(Verb::Display.chapter(), 0);
        assert_eq!(Verb::Sprout.chapter(), 1);
        assert_eq!(Verb::Compost.chapter(), 1);
        assert_eq!(Verb::Seek.chapter(), 2);
        assert_eq!(Verb::Sense.chapter(), 2);
    }

    #[test]
    fn bytes_roundtrip() {
        let e = TraceEvent::new(Verb::Produce, OK, 0xABCD, 0x1234);
        let back = TraceEvent::from_bytes(&e.to_bytes()).unwrap();
        assert_eq!(back.verb, 9);
        assert_eq!(back.chapter, 1);
        assert_eq!(back.outcome, OK);
        assert_eq!(back.target, 0xABCD);
        assert_eq!(back.timestamp, e.timestamp);
        assert!(back.provenance == 0x1234 || back.provenance == 0);
    }

    #[test]
    fn witness_never_empty() {
        let e = TraceEvent::new(Verb::Sense, E_TIMEOUT, 1, 2);
        let w = e.witness();
        assert!(!w.is_empty());
        assert!(w.contains("sense"));
        assert!(w.contains("-11"));
    }

    #[test]
    fn log_appends_binary_and_witness() {
        let path = std::env::temp_dir().join(format!("voxbox-trace-{}.trace", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut log = TraceLog::open(path.clone()).unwrap();
        let w1 = log.emit_ok(Verb::Sprout, 77).unwrap();
        let w2 = log.emit_fail(Verb::Produce, E_PERMISSION, 88).unwrap();
        assert!(!w1.is_empty());
        assert!(!w2.is_empty());
        let bytes = std::fs::read(&path).unwrap();
        let w1_line = w1.len() as usize + 1;
        assert_eq!(bytes.len(), 32 + w1_line + 32 + w2.len() as usize + 1);
        let first = TraceEvent::from_bytes(&bytes[0..32]).unwrap();
        assert_eq!(first.verb, Verb::Sprout.index());
        let second_offset = 32 + w1_line;
        let second = TraceEvent::from_bytes(&bytes[second_offset..second_offset + 32]).unwrap();
        assert_eq!(second.verb, Verb::Produce.index());
        std::fs::remove_file(&path).unwrap();
    }
}
