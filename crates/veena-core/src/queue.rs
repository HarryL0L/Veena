use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};

use crate::Track;

/// What happens when playback reaches the end of a track or the queue.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Repeat {
    /// Stop after the last track.
    #[default]
    Off,
    /// Start over from the first track.
    All,
    /// Replay the current track.
    One,
}

/// The ordered list of tracks to play.
///
/// `tracks` keeps the order the user sees. `order` is the play order: a list
/// of indices into `tracks`, which is the identity unless shuffle is on.
#[derive(Clone, Debug, Default)]
pub struct Queue {
    tracks: Vec<Track>,
    order: Vec<usize>,
    /// Position in `order` of the current track.
    pos: Option<usize>,
    repeat: Repeat,
    shuffle: bool,
}

impl Queue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    pub fn repeat(&self) -> Repeat {
        self.repeat
    }

    pub fn shuffle(&self) -> bool {
        self.shuffle
    }

    /// Index into [`Queue::tracks`] of the current track.
    pub fn current_index(&self) -> Option<usize> {
        self.pos.map(|p| self.order[p])
    }

    pub fn current(&self) -> Option<&Track> {
        self.current_index().map(|i| &self.tracks[i])
    }

    /// Add tracks to the end of the queue.
    pub fn extend(&mut self, tracks: impl IntoIterator<Item = Track>) {
        for track in tracks {
            self.order.push(self.tracks.len());
            self.tracks.push(track);
        }
    }

    pub fn push(&mut self, track: Track) {
        self.extend([track]);
    }

    /// Insert a track to play right after the current one.
    pub fn insert_next(&mut self, track: Track) {
        // Without shuffle, place it after the current track in the visible
        // list too, so the list matches the play order.
        let index = match (self.shuffle, self.current_index()) {
            (false, Some(current)) => current + 1,
            (false, None) => 0,
            (true, _) => self.tracks.len(),
        };
        for entry in &mut self.order {
            if *entry >= index {
                *entry += 1;
            }
        }
        self.tracks.insert(index, track);
        let at = self.pos.map_or(0, |p| p + 1);
        self.order.insert(at, index);
    }

    /// Remove the track at `index` (into [`Queue::tracks`]).
    ///
    /// If it was the current track, the following track becomes current.
    pub fn remove(&mut self, index: usize) -> Option<Track> {
        if index >= self.tracks.len() {
            return None;
        }
        let track = self.tracks.remove(index);
        let at = self.order.iter().position(|&i| i == index)?;
        self.order.remove(at);
        for entry in &mut self.order {
            if *entry > index {
                *entry -= 1;
            }
        }
        self.pos = match self.pos {
            Some(p) if at < p => Some(p - 1),
            Some(p) if p >= self.order.len() => None,
            other => other,
        };
        Some(track)
    }

    pub fn clear(&mut self) {
        self.tracks.clear();
        self.order.clear();
        self.pos = None;
    }

    /// Make the track at `index` (into [`Queue::tracks`]) current.
    pub fn jump(&mut self, index: usize) -> Option<&Track> {
        self.pos = Some(self.order.iter().position(|&i| i == index)?);
        self.current()
    }

    /// Advance to the next track.
    ///
    /// `manual` is true when the user pressed next. Then [`Repeat::One`] is
    /// ignored, because the user asked for a different track.
    /// Returns `None` at the end of the queue when repeat is off.
    pub fn next(&mut self, manual: bool) -> Option<&Track> {
        if self.order.is_empty() {
            return None;
        }
        let next = match self.pos {
            None => 0,
            Some(p) if self.repeat == Repeat::One && !manual => p,
            Some(p) if p + 1 < self.order.len() => p + 1,
            Some(_) if self.repeat == Repeat::Off => return None,
            Some(_) => {
                if self.shuffle {
                    self.order.shuffle(&mut rand::rng());
                }
                0
            }
        };
        self.pos = Some(next);
        self.current()
    }

    /// Go back to the previous track. Wraps around when repeat is on.
    pub fn previous(&mut self) -> Option<&Track> {
        let p = self.pos?;
        self.pos = Some(match p {
            0 if self.repeat == Repeat::Off => 0,
            0 => self.order.len() - 1,
            p => p - 1,
        });
        self.current()
    }

    pub fn set_repeat(&mut self, repeat: Repeat) {
        self.repeat = repeat;
    }

    /// Turn shuffle on or off. The current track keeps playing either way.
    pub fn set_shuffle(&mut self, shuffle: bool) {
        if shuffle == self.shuffle {
            return;
        }
        self.shuffle = shuffle;
        let current = self.current_index();
        if shuffle {
            let mut rest: Vec<usize> = (0..self.tracks.len())
                .filter(|&i| Some(i) != current)
                .collect();
            rest.shuffle(&mut rand::rng());
            self.order = current.into_iter().chain(rest).collect();
            self.pos = current.map(|_| 0);
        } else {
            self.order = (0..self.tracks.len()).collect();
            self.pos = current;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TrackId;

    fn track(name: &str) -> Track {
        Track::new(TrackId::new("test", name), name)
    }

    fn queue(names: &[&str]) -> Queue {
        let mut q = Queue::new();
        q.extend(names.iter().map(|n| track(n)));
        q
    }

    fn title(t: Option<&Track>) -> Option<&str> {
        t.map(|t| t.title.as_str())
    }

    #[test]
    fn plays_in_order_and_stops_at_end() {
        let mut q = queue(&["a", "b"]);
        assert_eq!(title(q.next(false)), Some("a"));
        assert_eq!(title(q.next(false)), Some("b"));
        assert_eq!(title(q.next(false)), None);
        assert_eq!(title(q.current()), Some("b"));
    }

    #[test]
    fn repeat_all_wraps() {
        let mut q = queue(&["a", "b"]);
        q.set_repeat(Repeat::All);
        q.next(false);
        q.next(false);
        assert_eq!(title(q.next(false)), Some("a"));
        assert_eq!(title(q.previous()), Some("b"));
    }

    #[test]
    fn repeat_one_replays_unless_manual() {
        let mut q = queue(&["a", "b"]);
        q.set_repeat(Repeat::One);
        q.next(false);
        assert_eq!(title(q.next(false)), Some("a"));
        assert_eq!(title(q.next(true)), Some("b"));
    }

    #[test]
    fn previous_stops_at_start() {
        let mut q = queue(&["a", "b"]);
        q.next(false);
        assert_eq!(title(q.previous()), Some("a"));
    }

    #[test]
    fn shuffle_keeps_current_and_visits_all() {
        let mut q = queue(&["a", "b", "c", "d", "e"]);
        q.jump(2);
        q.set_shuffle(true);
        assert_eq!(title(q.current()), Some("c"));
        let mut seen = vec!["c".to_string()];
        while let Some(t) = q.next(false) {
            seen.push(t.title.clone());
        }
        seen.sort();
        assert_eq!(seen, ["a", "b", "c", "d", "e"]);

        q.set_shuffle(false);
        assert_eq!(
            q.current_index().map(|i| q.tracks()[i].title.as_str()),
            title(q.current())
        );
    }

    #[test]
    fn insert_next_plays_after_current() {
        let mut q = queue(&["a", "b"]);
        q.next(false);
        q.insert_next(track("x"));
        assert_eq!(
            q.tracks()
                .iter()
                .map(|t| t.title.as_str())
                .collect::<Vec<_>>(),
            ["a", "x", "b"]
        );
        assert_eq!(title(q.next(false)), Some("x"));
        assert_eq!(title(q.next(false)), Some("b"));
    }

    #[test]
    fn remove_adjusts_current() {
        let mut q = queue(&["a", "b", "c"]);
        q.jump(1);
        q.remove(0);
        assert_eq!(title(q.current()), Some("b"));
        q.remove(0);
        assert_eq!(title(q.current()), Some("c"));
        q.remove(0);
        assert_eq!(q.current(), None);
        assert!(q.is_empty());
    }
}
