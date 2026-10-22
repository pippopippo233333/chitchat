//! Message search over the SQLite full-text index (`messages_fts`), limited to the channels
//! the caller can see and their own direct messages.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::Query;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::QueryBuilder;

use super::{ApiError, ApiResult, Auth};
use crate::db::{self, MESSAGE_SELECT};
use crate::models::MessageRow;
use crate::perms::{self, VIEW_CHANNEL};

const MAX_QUERY_CHARS: usize = 200;
const MAX_TERMS: usize = 16;

/// Turns what someone typed into a safe FTS5 query: every word must appear (accents and case
/// ignored), "quoted words" as a phrase, and the last word as a prefix while still typing.
/// Returns None when there are no words to look for.
pub fn fts_query(q: &str) -> Option<String> {
    let words = |s: &str| s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_string).collect::<Vec<_>>();
    let mut terms = Vec::new();
    for (i, part) in q.split('"').enumerate() {
        let w = words(part);
        if w.is_empty() {
            continue;
        }
        if i % 2 == 1 {
            terms.push(format!("\"{}\"", w.join(" ")));
        } else {
            terms.extend(w.into_iter().map(|w| format!("\"{w}\"")));
        }
    }
    terms.truncate(MAX_TERMS);
    // Prefix match on a last plain word that's still being typed (no space or quote after it).
    let open_word = q.chars().last().is_some_and(char::is_alphanumeric) && q.matches('"').count().is_multiple_of(2);
    if open_word && let Some(last) = terms.last_mut() {
        last.push('*');
    }
    (!terms.is_empty()).then(|| terms.join(" AND "))
}

/// Unix milliseconds, or a date `YYYY-MM-DD` (its start; with `end` the start of the next day),
/// or an RFC 3339 time.
fn parse_time(s: &str, end: bool) -> Option<i64> {
    if let Ok(ms) = s.parse::<i64>() {
        return Some(ms);
    }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let d = if end { d.succ_opt()? } else { d };
        return Some(d.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis());
    }
    chrono::DateTime::parse_from_rfc3339(s).ok().map(|t| t.timestamp_millis())
}

#[derive(Deserialize)]
pub struct SearchQuery {
    #[serde(default)]
    q: String,
    author_id: Option<String>,
    channel_id: Option<String>,
    from: Option<String>,
    to: Option<String>,
    before: Option<String>,
    limit: Option<i64>,
}

/// `GET /search`
pub async fn search(auth: Auth, Query(q): Query<SearchQuery>) -> ApiResult<Json<Value>> {
    let mut fields = BTreeMap::new();
    let id = |s: &Option<String>, field: &'static str, fields: &mut BTreeMap<&'static str, &'static str>| match s.as_deref().filter(|s| !s.is_empty()) {
        None => None,
        Some(s) => crate::ids::parse(s).or_else(|| {
            fields.insert(field, "invalid");
            None
        }),
    };
    let author = id(&q.author_id, "author_id", &mut fields);
    let channel = id(&q.channel_id, "channel_id", &mut fields);
    let before = id(&q.before, "before", &mut fields);
    let time = |s: &Option<String>, end: bool, field: &'static str, fields: &mut BTreeMap<&'static str, &'static str>| match s.as_deref().filter(|s| !s.is_empty()) {
        None => None,
        Some(s) => parse_time(s, end).or_else(|| {
            fields.insert(field, "invalid");
            None
        }),
    };
    let from = time(&q.from, false, "from", &mut fields);
    let to = time(&q.to, true, "to", &mut fields);
    if q.q.chars().count() > MAX_QUERY_CHARS {
        fields.insert("q", "too_long");
    }
    let text = fts_query(&q.q);
    if text.is_none() && author.is_none() && channel.is_none() && from.is_none() && to.is_none() {
        fields.insert("q", "empty");
    }
    if !fields.is_empty() {
        return Err(ApiError::validation(fields));
    }
    let limit = q.limit.unwrap_or(25).clamp(1, 50);

    let mut qb = QueryBuilder::new(MESSAGE_SELECT);
    qb.push(" WHERE m.channel_id IN (SELECT id FROM channels WHERE (type = 'text' AND ");
    qb.push_bind(perms::has(auth.perms, VIEW_CHANNEL));
    qb.push(") OR (type = 'dm' AND ");
    qb.push_bind(auth.user.id);
    qb.push(" IN (dm_a, dm_b)))");
    if let Some(text) = &text {
        qb.push(" AND m.id IN (SELECT rowid FROM messages_fts WHERE messages_fts MATCH ");
        qb.push_bind(text);
        qb.push(")");
    }
    if let Some(a) = author {
        qb.push(" AND m.author_id = ").push_bind(a);
    }
    if let Some(c) = channel {
        qb.push(" AND m.channel_id = ").push_bind(c);
    }
    if let Some(f) = from {
        qb.push(" AND m.created_at >= ").push_bind(f);
    }
    if let Some(t) = to {
        qb.push(" AND m.created_at < ").push_bind(t);
    }
    if let Some(b) = before {
        qb.push(" AND m.id < ").push_bind(b);
    }
    qb.push(" ORDER BY m.id DESC LIMIT ").push_bind(limit + 1);
    let mut rows: Vec<MessageRow> = qb.build_query_as().fetch_all(&auth.srv.db).await?;
    let more = rows.len() as i64 > limit;
    rows.truncate(limit as usize);
    let next = if more { rows.last().map(|m| m.id.to_string()) } else { None };
    let results = db::messages_out(&auth.srv, &rows, auth.user.id).await?;
    Ok(Json(json!({ "results": results, "next_before": next })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queries() {
        assert_eq!(fts_query("ciao mon").as_deref(), Some("\"ciao\" AND \"mon\"*"));
        assert_eq!(fts_query("ciao mondo ").as_deref(), Some("\"ciao\" AND \"mondo\""));
        assert_eq!(fts_query("\"buon giorno\" a").as_deref(), Some("\"buon giorno\" AND \"a\"*"));
        assert_eq!(fts_query("\"buon giorno\"").as_deref(), Some("\"buon giorno\""));
        // FTS syntax is never passed through.
        assert_eq!(fts_query("a OR b NOT c*").as_deref(), Some("\"a\" AND \"OR\" AND \"b\" AND \"NOT\" AND \"c\""));
        assert_eq!(fts_query("  ...  "), None);
        assert_eq!(fts_query(""), None);
    }

    #[test]
    fn times() {
        assert_eq!(parse_time("1700000000000", false), Some(1_700_000_000_000));
        assert_eq!(parse_time("2026-10-22", false), Some(1_792_627_200_000));
        assert_eq!(parse_time("2026-10-22", true), Some(1_792_713_600_000));
        assert!(parse_time("yesterday", false).is_none());
    }
}
