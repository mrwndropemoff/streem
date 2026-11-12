//! What is playing, as plain functions over [`State`]. No player is involved: the player draws
//! `State.playback` and reports back with [`PlaybackReport`]s.
//!
//! Nothing here logs. A stream's link can hold a **debrid service** key.

use crate::{Details, Load, Playback, PlaybackReport, SeekRequest, State, SubtitleTrack};

/// The notice raised when the player could not play a stream.
pub(crate) const FAILED_NOTICE: &str = "This stream could not be played. Pick another one.";

/// The link of every stream in a stream list, in the order of the list's groups and of the
/// streams in each group. `None` for a stream that cannot be played.
pub(crate) type Links = Vec<Vec<Option<String>>>;

/// What a report changed, which decides whether the UI hears about it right away.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Reported {
    /// Nothing was playing.
    Ignored,
    /// Only the position moved. Many of these arrive, so they are not all published.
    PositionOnly,
    /// The duration or buffering changed, or playback ended.
    Changed,
    /// The stream could not be played. The caller raises [`FAILED_NOTICE`].
    Failed,
}

/// Starts playing a stream of the open title. Does nothing when there is no such stream or it
/// cannot be played. Returns whether a new playback started.
pub(crate) fn play(state: &mut State, links: &Links, addon_id: &str, stream_index: usize) -> bool {
    let Some(playback) = start(state, links, addon_id, stream_index) else {
        return false;
    };
    state.playback = Some(playback);
    true
}

/// Adds the tracks one **addon** offers to what is playing. The caller has already checked
/// that they were asked for this playback.
pub(crate) fn add_subtitles(state: &mut State, tracks: Vec<SubtitleTrack>) {
    if let Some(playback) = state.playback.as_mut() {
        playback.subtitles.extend(tracks);
    }
}

fn start(state: &State, links: &Links, addon_id: &str, stream_index: usize) -> Option<Playback> {
    let details = state.details.as_ref()?;
    let list = details.streams.as_ref()?;
    let group_index = list
        .groups
        .iter()
        .position(|group| group.addon_id == addon_id)?;
    let Load::Ready(items) = &list.groups[group_index].streams else {
        return None;
    };
    let item = items.get(stream_index).filter(|item| item.playable)?;
    let url = links.get(group_index)?.get(stream_index)?.clone()?;

    Some(Playback {
        meta_type: details.meta_type.clone(),
        meta_id: details.meta_id.clone(),
        video_id: list.video_id.clone(),
        title: title(details, &list.video_id),
        poster: match &details.meta {
            Load::Ready(meta) => meta.poster.clone(),
            _ => None,
        },
        addon_id: addon_id.to_string(),
        stream_name: item.name.clone(),
        filename: item.filename.clone(),
        url,
        paused: false,
        seek: None,
        position_secs: 0.0,
        duration_secs: 0.0,
        buffering: false,
        subtitles: Vec::new(),
    })
}

/// "Name", or for an episode "Name · S1E2 · Episode title".
fn title(details: &Details, video_id: &str) -> String {
    let Load::Ready(meta) = &details.meta else {
        return details.meta_id.clone();
    };
    let episode = meta.videos.iter().find(|video| video.id == video_id);
    match episode.and_then(|video| Some((video, video.season?, video.episode?))) {
        Some((video, season, number)) if video.title.is_empty() => {
            format!("{} · S{season}E{number}", meta.name)
        }
        Some((video, season, number)) => {
            format!("{} · S{season}E{number} · {}", meta.name, video.title)
        }
        None => meta.name.clone(),
    }
}

pub(crate) fn set_paused(state: &mut State, paused: bool) {
    if let Some(playback) = state.playback.as_mut() {
        playback.paused = paused;
    }
}

/// Asks for a jump, kept within `0..=duration` once the duration is known. The position shows
/// the target at once, so a slider does not jump back until the player reports.
pub(crate) fn seek(state: &mut State, position_secs: f64) {
    let Some(playback) = state.playback.as_mut() else {
        return;
    };
    if position_secs.is_nan() {
        return;
    }
    let longest = if playback.duration_secs > 0.0 {
        playback.duration_secs
    } else {
        f64::INFINITY
    };
    let position_secs = position_secs.clamp(0.0, longest);
    let id = playback.seek.as_ref().map_or(0, |seek| seek.id) + 1;
    playback.seek = Some(SeekRequest { id, position_secs });
    playback.position_secs = position_secs;
}

pub(crate) fn stop(state: &mut State) {
    state.playback = None;
}

/// Applies what the player said. A number that is not finite is a player glitch and is
/// ignored, so it cannot make the state differ from itself on every report.
pub(crate) fn report(state: &mut State, report: PlaybackReport) -> Reported {
    let Some(playback) = state.playback.as_mut() else {
        return Reported::Ignored;
    };
    if report.failed.is_some() {
        state.playback = None;
        return Reported::Failed;
    }
    if report.ended {
        state.playback = None;
        return Reported::Changed;
    }

    let duration_secs = if report.duration_secs.is_finite() && report.duration_secs >= 0.0 {
        report.duration_secs
    } else {
        playback.duration_secs
    };
    if report.position_secs.is_finite() {
        playback.position_secs = report.position_secs.max(0.0);
    }
    let changed = playback.duration_secs != duration_secs || playback.buffering != report.buffering;
    playback.duration_secs = duration_secs;
    playback.buffering = report.buffering;
    if changed {
        Reported::Changed
    } else {
        Reported::PositionOnly
    }
}
