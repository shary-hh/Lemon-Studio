// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The piano roll's scale and chord helpers (the toolbar's Blend button): the key, the scale to highlight on the
//! keyboard and in the grid, the chords to insert at the play-start marker, and fitting the notes to the scale. The
//! scales and chords are FL's `Chords.map` entries (`crate::chords`), as the Limit dialog lists them; the menu is
//! `tools_menu` and the drawing is in `views/piano.rs`.

use crate::app::State;
use crate::chords;
use crate::piano;
use crate::piano_tools::NCmd;
use crate::playlist::{Cmd, Mark, PItem, Popup};

/// The pitch-class names of the twelve keys, from C.
pub const ROOTS: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// How many chords the Chords.map holds (entries 1..=66).
pub const CHORD_COUNT: usize = chords::FIRST_SCALE - 1;

/// How many scales follow them (entries 67..=88).
pub const SCALE_COUNT: usize = chords::ENTRIES.len() - chords::FIRST_SCALE + 1;

/// What a click in the piano roll stamps while a helper is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stamp {
    /// A chord, by entry in the Chords.map (1..=66).
    Chord(u8),
    /// A scale, by index in the scale list (0 = Major).
    Scale(u8),
}

impl Stamp {
    /// The menu label: the chord's or scale's name.
    pub fn label(self) -> String {
        match self {
            Stamp::Chord(k) => chords::label(usize::from(k).clamp(1, CHORD_COUNT)).to_string(),
            Stamp::Scale(i) => scale_label(usize::from(i)).to_string(),
        }
    }
}

/// A scale's name, by index in the scale list (0 = Major).
pub fn scale_label(i: usize) -> &'static str {
    chords::label(chords::FIRST_SCALE + i.min(SCALE_COUNT - 1))
}

/// The pitch classes of the highlighted scale at the chosen key, if one is highlighted.
pub fn scale_classes(s: &State) -> Option<[bool; 12]> {
    let i = s.piano.scale?;

    Some(chords::classes(chords::FIRST_SCALE + usize::from(i).min(SCALE_COUNT - 1), s.piano.key_root % 12))
}

/// The root's pitch class (0 = C) of the scale or chord the helpers have chosen, if there is one: the key's root,
/// marked on the piano roll's keyboard (and in its grid) so that the tonal centre shows.
pub fn root_class(s: &State) -> Option<u8> {
    (s.piano.scale.is_some() || s.piano.stamp.is_some()).then_some(s.piano.key_root % 12)
}

/// The keys of chord `k` (1..=66) in root position from `base` (a key number), one per note.
fn chord_keys(k: usize, base: i32) -> Vec<i32> {
    let mut keys = vec![base];
    let mut offset = 0i32;
    if let Some(entry) = chords::entry(k.clamp(1, CHORD_COUNT)) {
        for &interval in entry.intervals {
            offset += i32::from(interval);
            keys.push(base + offset);
        }
    }
    keys
}

/// One octave of scale `i` from the first key at or above `base` that the (key `key_root`) scale holds; the octave's
/// top note closes the run.
fn scale_keys(i: usize, key_root: u8, base: i32) -> Vec<i32> {
    let on = chords::classes(chords::FIRST_SCALE + i.min(SCALE_COUNT - 1), key_root % 12);
    let mut keys = Vec::new();
    let mut key = base;
    while key <= base + 12 {
        if on[key.rem_euclid(12) as usize] {
            keys.push(key);
        }
        key += 1;
    }
    keys
}

/// The keys a stamp writes for `stamp` from `base`: a chord's notes, or one octave of a scale.
pub fn stamp_keys(stamp: Stamp, key_root: u8, base: i32) -> Vec<i32> {
    let keys = match stamp {
        Stamp::Chord(k) => chord_keys(usize::from(k), base),
        Stamp::Scale(i) => scale_keys(usize::from(i), key_root, base),
    };
    keys.into_iter().filter(|&k| (0..=131).contains(&k)).collect()
}

/// Stamp `stamp` into the current pattern at `tick`/`key`, as one undo step; selected. Each note starts a grid step
/// after the one before while the helpers are set to diagonal (the default), and lasts that step, so the stamp shows
/// as a diagonal run; otherwise the notes all start at `tick`. Returns the new notes' indices (empty without a
/// channel or at the edges of the keyboard).
pub fn stamp(s: &mut State, stamp: Stamp, tick: u32, key: i32) -> Vec<usize> {
    let Some(ch) = piano::channel(s) else { return Vec::new() };
    let step = crate::piano_snap::unit(s).unwrap_or_else(|| u32::from(s.project.ppq.max(4)) / 4).max(1);
    let diagonal = s.piano.stamp_diagonal;
    let keys = stamp_keys(stamp, s.piano.key_root, key);
    if keys.is_empty() {
        return Vec::new();
    }
    piano::snapshot_as(s, "Piano roll stamp");
    let mut added = Vec::with_capacity(keys.len());
    for (i, key) in keys.into_iter().enumerate() {
        let at = tick + if diagonal { i as u32 * step } else { 0 };
        let index = piano::add_note(s, ch, at, key);
        if diagonal {
            piano::pattern_mut(s).notes[index].length = step;
        }
        added.push(index);
    }
    s.piano.last_len = if diagonal { step } else { s.piano.last_len };
    s.piano.select(s.pattern, ch, added.clone());
    s.piano.dirty = true;
    added
}

/// Insert chord `k` (1..=66) at the play-start marker (or the time selection's start, snapped down to the snap grid)
/// on the piano roll's channel, as one undo step; the new notes are selected. Returns the status message.
pub fn insert_chord(s: &mut State, k: usize) -> String {
    let Some(ch) = piano::channel(s) else {
        return "Piano roll: no channel to insert the chord into".into();
    };
    let k = k.clamp(1, CHORD_COUNT);
    let start = s.piano.range.map_or(s.piano.marker, |(a, _)| a);
    let start = crate::piano_snap::unit(s).map_or(start, |u| start - start % u.max(1));
    let root = i32::from(s.piano.key_root % 12);
    piano::snapshot_as(s, "Piano roll insert chord");
    let mut added = Vec::new();
    for key in chord_keys(k, 60 + root) {
        added.push(piano::add_note(s, ch, start, key));
    }
    s.piano.select(s.pattern, ch, added);
    s.piano.dirty = true;
    format!("Piano roll: {} in {} at the play start", chords::label(k), ROOTS[root as usize])
}

/// Move each target note to the nearest key of the highlighted scale, as one undo step. False when there is no scale
/// or nothing changed.
pub fn fit(s: &mut State) -> bool {
    let Some(classes) = scale_classes(s) else { return false };
    crate::piano_tools::apply_as(
        s,
        move |_, notes| {
            notes
                .into_iter()
                .map(|mut n| {
                    let key = i32::from(n.key);
                    let found = (0..=6)
                        .flat_map(|d| if d == 0 { vec![key] } else { vec![key - d, key + d] })
                        .find(|&k| (0..=131).contains(&k) && classes[k.rem_euclid(12) as usize]);
                    if let Some(k) = found {
                        n.key = k as u16;
                    }
                    n
                })
                .collect()
        },
        Some("Piano roll fit to scale"),
    )
}

/// Select the notes of the piano roll's channel whose pitch class is in the highlighted scale. Returns how many
/// were selected (0 keeps the selection empty).
pub fn select_scale(s: &mut State) -> usize {
    let Some(classes) = scale_classes(s) else { return 0 };
    select_pitch_classes(s, &classes)
}

/// Select the notes of the piano roll's channel whose pitch class is in the chord set to stamp. Returns how many.
pub fn select_chord(s: &mut State) -> usize {
    let Some(Stamp::Chord(k)) = s.piano.stamp else { return 0 };
    let classes = chords::classes(usize::from(k).clamp(1, CHORD_COUNT), s.piano.key_root % 12);
    select_pitch_classes(s, &classes)
}

fn select_pitch_classes(s: &mut State, classes: &[bool; 12]) -> usize {
    let Some(ch) = piano::channel(s) else { return 0 };
    let selected: Vec<usize> = piano::notes(s)
        .iter()
        .enumerate()
        .filter(|(_, n)| n.channel == ch && classes[i32::from(n.key).rem_euclid(12) as usize])
        .map(|(i, _)| i)
        .collect();
    let count = selected.len();
    s.piano.select(s.pattern, ch, selected);
    count
}

/// The toolbar button's menu: the key, the scale to highlight, the chords to insert and Fit notes to scale.
pub fn tools_menu(s: &State, x: f32, y: f32) -> Popup {
    let root = s.piano.key_root % 12;
    let keys = ROOTS
        .iter()
        .enumerate()
        .map(|(i, name)| PItem::new(*name, Cmd::Piano(NCmd::SetRoot(i as u8))).mark(Mark::Radio(usize::from(root) == i)))
        .collect();
    let mut scales = vec![PItem::new("Off", Cmd::Piano(NCmd::SetScale(None))).mark(Mark::Radio(s.piano.scale.is_none()))];
    scales.extend((0..SCALE_COUNT).map(|i| {
        let index = i as u8;
        PItem::new(scale_label(i), Cmd::Piano(NCmd::SetScale(Some(index)))).mark(Mark::Radio(s.piano.scale == Some(index)))
    }));
    // Chords keep the file's own separators (entries 10, 15, 40, 55 and 60 begin a new group).
    let mut chords_menu = Vec::with_capacity(CHORD_COUNT + chords::SEPARATED.len());
    for k in 1..=CHORD_COUNT {
        if k > 1 && chords::SEPARATED.contains(&k) {
            chords_menu.push(PItem::sep());
        }
        chords_menu.push(PItem::new(chords::label(k), Cmd::Piano(NCmd::InsertChord(k as u8))));
    }
    let fit = PItem::new("Fit notes to scale", Cmd::Piano(NCmd::FitScale));
    let fit = if s.piano.scale.is_some() { fit } else { fit.off() };
    let scale_name = s.piano.scale.map_or_else(|| "off".to_string(), |i| scale_label(usize::from(i)).to_string());
    // Stamping with the mouse: one chord or one scale; the diagonal switch spreads the notes one grid step apart.
    let mut chord_stamp = vec![PItem::new("Off", Cmd::Piano(NCmd::StampChord(None))).mark(Mark::Radio(!matches!(s.piano.stamp, Some(Stamp::Chord(_)))))];
    chord_stamp.extend((1..=CHORD_COUNT).map(|k| {
        let index = k as u8;
        PItem::new(chords::label(k), Cmd::Piano(NCmd::StampChord(Some(index)))).mark(Mark::Radio(s.piano.stamp == Some(Stamp::Chord(index))))
    }));
    let mut scale_stamp = vec![PItem::new("Off", Cmd::Piano(NCmd::StampScale(None))).mark(Mark::Radio(!matches!(s.piano.stamp, Some(Stamp::Scale(_)))))];
    scale_stamp.extend((0..SCALE_COUNT).map(|i| {
        let index = i as u8;
        PItem::new(scale_label(i), Cmd::Piano(NCmd::StampScale(Some(index)))).mark(Mark::Radio(s.piano.stamp == Some(Stamp::Scale(index))))
    }));
    let chord_name = match s.piano.stamp {
        Some(Stamp::Chord(k)) => chords::label(usize::from(k).clamp(1, CHORD_COUNT)).to_string(),
        _ => "off".to_string(),
    };
    let stamped_scale = match s.piano.stamp {
        Some(Stamp::Scale(i)) => scale_label(usize::from(i)).to_string(),
        _ => "off".to_string(),
    };
    // Selecting the notes that belong to the scale or chord. They are greyed out until there is one to select by.
    let select_scale = PItem::new("Notes of the scale", Cmd::Piano(NCmd::SelectScale));
    let select_scale = if s.piano.scale.is_some() { select_scale } else { select_scale.off() };
    let select_chord = PItem::new("Notes of the chord", Cmd::Piano(NCmd::SelectChord));
    let select_chord = if matches!(s.piano.stamp, Some(Stamp::Chord(_))) { select_chord } else { select_chord.off() };
    let items = vec![
        PItem::head("Chords and scales"),
        PItem::sub(format!("Key: {}", ROOTS[usize::from(root)]), keys),
        PItem::sub(format!("Highlight scale: {scale_name}"), scales),
        PItem::head("Stamp with the mouse"),
        PItem::sub(format!("Chord: {chord_name}"), chord_stamp),
        PItem::sub(format!("Scale: {stamped_scale}"), scale_stamp),
        PItem::new("Diagonal (one step apart)", Cmd::Piano(NCmd::StampDiagonal(!s.piano.stamp_diagonal))).mark(Mark::Check(s.piano.stamp_diagonal)),
        PItem::head("Select the notes of..."),
        select_scale,
        select_chord,
        fit,
        PItem::head("At the play start"),
        PItem::sub("Insert chord", chords_menu),
    ];
    Popup { x, y, items, sub: None, scroll: [0; 2] }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Chords.map holds 66 chords and 22 scales, numbered as its file is.
    #[test]
    fn counts_and_labels() {
        assert_eq!((CHORD_COUNT, SCALE_COUNT), (66, 22));
        assert_eq!(scale_label(0), "Major");
        assert_eq!(scale_label(SCALE_COUNT - 1), "Locrian");
        assert_eq!(ROOTS[0], "C");
        assert_eq!(ROOTS[11], "B");
    }

    /// A chord's keys are its intervals above the base, in root position; a scale runs one octave from the clicked key.
    #[test]
    fn chord_keys_follow_the_intervals() {
        assert_eq!(chord_keys(1, 60), [60, 64, 67]);
        assert_eq!(chord_keys(5, 60), [60, 63, 67]);
        assert_eq!(chord_keys(25, 60), [60, 64, 67, 70, 81]);
    }

    /// The stamp keys: a chord as it sounds, a scale from the nearest key of the chosen key's scale up to its octave.
    #[test]
    fn stamp_keys_are_chords_and_scales() {
        assert_eq!(stamp_keys(Stamp::Chord(1), 0, 60), [60, 64, 67]);
        // C major from D4: D E F G A B C D5.
        assert_eq!(stamp_keys(Stamp::Scale(0), 0, 62), [62, 64, 65, 67, 69, 71, 72, 74]);
        // D major from D4: D E F# G A B C# D5.
        assert_eq!(stamp_keys(Stamp::Scale(0), 2, 62), [62, 64, 66, 67, 69, 71, 73, 74]);
    }

    /// Highlighting a scale follows the key: C major and D major differ as the file says.
    #[test]
    fn scale_classes_follow_the_key() {
        assert_eq!(chords::classes(chords::FIRST_SCALE, 0), [true, false, true, false, true, true, false, true, false, true, false, true]);
        let d = chords::classes(chords::FIRST_SCALE, 2);
        assert_eq!((0..12).filter(|&i| d[i]).collect::<Vec<_>>(), [1, 2, 4, 6, 7, 9, 11]);
    }
}
