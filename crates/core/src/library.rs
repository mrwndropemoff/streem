//! The library: titles the person keeps, as plain functions over [`State`]. Saving is done by
//! local storage; this decides what to save and keeps `State` in step with what was saved.

use crate::{LibraryItem, Load, State};

/// What [`crate::Command::AddToLibrary`] saves: the open title, once its details are ready.
/// Keyed by the type and id the title was opened with, so opening it from the library asks
/// for the same title again.
pub(crate) fn item_to_add(state: &State) -> Option<LibraryItem> {
    let details = state.details.as_ref()?;
    let Load::Ready(meta) = &details.meta else {
        return None;
    };
    Some(LibraryItem {
        meta_type: details.meta_type.clone(),
        meta_id: details.meta_id.clone(),
        name: meta.name.clone(),
        poster: meta.poster.clone(),
    })
}

/// Replaces the library with what local storage now holds.
pub(crate) fn set(state: &mut State, items: Vec<LibraryItem>) {
    state.library = items;
    mark_details(state);
}

/// Makes `Details::in_library` match the library. Called when a title opens and whenever the
/// library changes.
pub(crate) fn mark_details(state: &mut State) {
    if let Some(details) = state.details.as_mut() {
        details.in_library = state
            .library
            .iter()
            .any(|item| item.meta_id == details.meta_id);
    }
}
