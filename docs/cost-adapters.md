# Declarative cost reports

An optional `[costs]` mapping declares a read-only organization spend report. It
requires a GET-only adapter without balance or quota mappings. The engine uses
`start_parameter`, `end_parameter`, and `time_format` (`unix_seconds` or `rfc3339`)
to request the last 30 completed UTC days. `query` holds public fixed parameters.
`rows` locates the array; optional `nested_rows` locates entries within each bucket.
`amount` and `currency` are JSON pointers relative to each entry. `divisor` converts
source units into USD (1 for dollars, 100 for cents). USD currency must be explicit.

`has_more`, `next_page`, and `page_parameter` describe pagination. Up to 16 pages
of 1 MiB each are read; missing pagination fields, repeated cursors, malformed
amounts, unexpected currency, and incomplete pages fail the whole refresh. Partial
costs are not returned. An empty result stays unavailable; an explicit zero amount
stays zero. Signed amounts are retained to support adjustments. Cache data includes
the exact reporting window and uses existing stale/error semantics.

Optional `[headers]` contains public API version metadata only. Header names must
end in `-version` or `-beta`, be valid HTTP header names, and differ from the auth
header. Credentials still use the separate existing credential mechanism. Empty
new fields are omitted during serialization to preserve existing account bindings.
