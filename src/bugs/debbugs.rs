use super::{BugCache, BugRow, CachedDebbugsBugDetails};

/// Debian bug data returned to completion providers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebbugsBugSummary {
    /// Numeric Debian bug ID.
    pub id: u32,
    /// Bug title, when available.
    pub title: Option<String>,
    /// Bug severity (e.g. "serious", "normal", "wishlist").
    pub severity: Option<String>,
    /// Whether the bug has been marked as done/resolved.
    pub done: bool,
    /// Tags associated with the bug (e.g. "patch", "confirmed").
    pub tags: Option<String>,
    /// Where the bug has been forwarded to, if anywhere.
    pub forwarded: Option<String>,
    /// Email address of the person who reported the bug.
    pub originator: Option<String>,
}

impl BugCache {
    /// Fetch bug IDs and details for a package from UDD in a single query.
    async fn fetch_bugs_for_package(&mut self, package: &str) {
        if self.bug_ids_by_package.contains_key(package) {
            return;
        }

        let rows: Vec<BugRow> = match sqlx::query_as(
            "SELECT b.id, b.title, b.severity::text, b.done, b.forwarded, b.submitter, \
                    (SELECT string_agg(t.tag, ', ') FROM bugs_tags t WHERE t.id = b.id) AS tags \
             FROM bugs b \
             WHERE b.source = $1 \
             ORDER BY b.id",
        )
        .bind(package)
        .fetch_all(&*self.pool)
        .await
        {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!(package, error = %e, "UDD bug query failed");
                return;
            }
        };

        let mut ids = Vec::new();
        for row in rows {
            let Some(id) = u32::try_from(row.id).ok() else {
                continue;
            };
            ids.push(id);
            self.bug_details_by_id.insert(
                id,
                CachedDebbugsBugDetails {
                    title: row.title,
                    severity: row.severity,
                    done: row.done.as_ref().is_some_and(|d| !d.is_empty()),
                    tags: row.tags,
                    forwarded: row.forwarded,
                    originator: row.submitter,
                },
            );
        }

        self.bug_ids_by_package.insert(package.to_string(), ids);
    }

    /// Return Debian bug summaries for `package` that match a decimal prefix.
    pub async fn get_bug_summaries_with_prefix(
        &mut self,
        package: &str,
        prefix: &str,
    ) -> Vec<DebbugsBugSummary> {
        self.fetch_bugs_for_package(package).await;

        let normalized_prefix = prefix.trim();
        let Some(ids) = self.bug_ids_by_package.get(package) else {
            return Vec::new();
        };

        ids.iter()
            .filter(|id| id.to_string().starts_with(normalized_prefix))
            .map(|&id| self.make_summary(id))
            .collect()
    }

    fn make_summary(&self, id: u32) -> DebbugsBugSummary {
        match self.bug_details_by_id.get(&id) {
            Some(details) => DebbugsBugSummary {
                id,
                title: details.title.clone(),
                severity: details.severity.clone(),
                done: details.done,
                tags: details.tags.clone(),
                forwarded: details.forwarded.clone(),
                originator: details.originator.clone(),
            },
            None => DebbugsBugSummary {
                id,
                title: None,
                severity: None,
                done: false,
                tags: None,
                forwarded: None,
                originator: None,
            },
        }
    }

    /// Pre-fetch open bug IDs and their details for a package.
    ///
    /// Call this in the background so the data is cached before the user
    /// triggers completion.
    pub async fn prefetch_bugs_for_package(&mut self, package: &str) {
        self.fetch_bugs_for_package(package).await;
    }

    #[cfg(test)]
    pub(crate) fn insert_cached_open_bugs_for_package(
        &mut self,
        package: &str,
        bugs: Vec<(u32, Option<&str>)>,
    ) {
        let mut sorted_unique_ids = std::collections::BTreeSet::new();

        for (id, title) in bugs {
            sorted_unique_ids.insert(id);
            self.bug_details_by_id.insert(
                id,
                CachedDebbugsBugDetails {
                    title: title.map(ToString::to_string),
                    severity: None,
                    done: false,
                    tags: None,
                    forwarded: None,
                    originator: None,
                },
            );
        }

        self.bug_ids_by_package
            .insert(package.to_string(), sorted_unique_ids.into_iter().collect());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_bug_summaries_with_prefix_from_cache() {
        let mut cache = BugCache::new(crate::udd::shared_pool());
        cache.insert_cached_open_bugs_for_package(
            "foo",
            vec![
                (123456, Some("Fix crash on startup")),
                (123499, None),
                (888888, Some("Unrelated issue")),
            ],
        );

        let summaries = cache.get_bug_summaries_with_prefix("foo", "1234").await;
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].id, 123456);
        assert_eq!(summaries[0].title.as_deref(), Some("Fix crash on startup"));
        assert_eq!(summaries[1].id, 123499);
        assert_eq!(summaries[1].title, None);
    }

    #[tokio::test]
    #[ignore] // requires network access to UDD
    async fn test_fetch_bugs_from_udd() {
        let mut cache = BugCache::new(crate::udd::shared_pool());
        let summaries = cache.get_bug_summaries_with_prefix("lintian", "").await;
        assert!(!summaries.is_empty(), "lintian should have bugs in UDD");
        // Every summary should have a title
        assert!(
            summaries.iter().any(|s| s.title.is_some()),
            "at least some bugs should have titles"
        );
    }
}
