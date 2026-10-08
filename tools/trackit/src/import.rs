//! Importing events into topics and the import summary text.

use crate::app::{TopicList, new_topic_signal};
use crate::db::{
    EventRow, TopicHeader, add_events_bulk_idb, load_events_for_topic, save_topic_header,
};
use crate::time::{event_row_counts, merge_new_events, new_id, time_boundaries};
use leptos::prelude::*;
use rexie::Rexie;

/// Names of all topics except the one with id `except` (pass "" for none).
pub(crate) fn other_topic_names(topic_list: TopicList, except: &str) -> Vec<String> {
    topic_list.with_untracked(|rows| {
        rows.iter()
            .filter_map(|s| s.with_untracked(|h| (h.id != except).then(|| h.name.clone())))
            .collect()
    })
}

/// Position for a newly created topic: after all existing ones.
pub(crate) fn next_position(topic_list: TopicList) -> u32 {
    topic_list.with_untracked(|rows| {
        rows.iter()
            .map(|s| s.with_untracked(|h| h.position))
            .max()
            .map_or(0, |max| max + 1)
    })
}

// ─── Import helper ────────────────────────────────────────────────────────────

/// What importing one topic's events did.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ImportOutcome {
    pub added: usize,
    pub duplicates: usize,
}

/// Merge `incoming` into the topic called `name`, creating the topic if it
/// does not exist. Events already stored (same timestamp) are skipped; the
/// new ones get fresh ids and are written in a single transaction, after
/// which the topic's counts are recomputed. `None` if the write failed.
pub(crate) async fn import_into_topic(
    db: &Rexie,
    topic_list: TopicList,
    app_owner: StoredValue<Owner, LocalStorage>,
    name: String,
    incoming: Vec<EventRow>,
) -> Option<ImportOutcome> {
    let existing_sig = topic_list.with_untracked(|rows| {
        rows.iter()
            .find(|s| s.with_untracked(|h| h.name == name))
            .copied()
    });
    let (topic_id, existing) = match existing_sig {
        Some(sig) => {
            let id = sig.with_untracked(|h| h.id.clone());
            let events = load_events_for_topic(db, &id).await;
            (id, events)
        }
        None => (new_id(), Vec::new()),
    };

    let (mut fresh, duplicates) = merge_new_events(&existing, incoming);
    for row in &mut fresh {
        row.id = new_id();
        row.topic_id = topic_id.clone();
    }
    if !add_events_bulk_idb(db, &fresh).await {
        return None;
    }
    let added = fresh.len();
    let mut all = existing;
    all.extend(fresh);
    let counts = event_row_counts(&all, time_boundaries());

    match existing_sig {
        Some(sig) => {
            sig.update(|h| h.set_counts(counts));
            save_topic_header(db, &sig.get_untracked()).await;
        }
        None => {
            let mut header = TopicHeader {
                position: next_position(topic_list),
                ..TopicHeader::new(topic_id, name)
            };
            header.set_counts(counts);
            save_topic_header(db, &header).await;
            let sig = app_owner.with_value(|o| new_topic_signal(o, header));
            topic_list.update(|rows| rows.push(sig));
        }
    }
    Some(ImportOutcome { added, duplicates })
}

impl std::ops::Add for ImportOutcome {
    type Output = ImportOutcome;
    fn add(self, o: ImportOutcome) -> ImportOutcome {
        ImportOutcome {
            added: self.added + o.added,
            duplicates: self.duplicates + o.duplicates,
        }
    }
}

/// "1 event", "3 events".
pub(crate) fn count_noun(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

/// Toast text after an import, e.g.
/// "Imported 120 events into Running (15 duplicates skipped)".
pub(crate) fn import_message(scope: &str, o: ImportOutcome) -> String {
    let mut msg = format!("Imported {} {scope}", count_noun(o.added, "event"));
    if o.duplicates > 0 {
        msg.push_str(&format!(
            " ({} skipped)",
            count_noun(o.duplicates, "duplicate")
        ));
    }
    msg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_noun_singular_and_plural() {
        assert_eq!(count_noun(0, "topic"), "0 topics");
        assert_eq!(count_noun(1, "topic"), "1 topic");
        assert_eq!(count_noun(3, "event"), "3 events");
    }

    #[test]
    fn import_message_with_duplicates() {
        let o = ImportOutcome {
            added: 120,
            duplicates: 15,
        };
        assert_eq!(
            import_message("into Running", o),
            "Imported 120 events into Running (15 duplicates skipped)"
        );
    }

    #[test]
    fn import_message_singular() {
        let o = ImportOutcome {
            added: 1,
            duplicates: 1,
        };
        assert_eq!(
            import_message("into Yoga", o),
            "Imported 1 event into Yoga (1 duplicate skipped)"
        );
    }

    #[test]
    fn import_message_without_duplicates() {
        let o = ImportOutcome {
            added: 2,
            duplicates: 0,
        };
        assert_eq!(
            import_message("from 2 topics", o),
            "Imported 2 events from 2 topics"
        );
    }
}
