# Planning: Gallery `short_id`, Lookup Endpoint & Raw Image Serving

This document describes the backend work required to support the frontend Gallery
feature described in User Gallery Routes, Image Serving, Zoom Modal & Raw Image View.

It builds on the existing Rust (Axum + MySQL + SQLx) backend. The focus is the
Gallery resource only. The document is high level — component/function names and
exact SQL are left to the implementer, but must match the existing code style in
`src/handlers/gallery.rs`, `src/media.rs`, and `src/db.rs`.

## Background & Dependency

The frontend needs shareable, unguessable image URLs of the form
`/:username/gallery/:shortId`, where `shortId` is an **8-character random string**.
The current backend identifies gallery images only by a numeric auto-increment
`id` and does not expose a `short_id`. The frontend flagged this as a backend
dependency.

This issue adds first-class `short_id` support so the frontend does not have to
maintain a fragile client-side id mapping, plus a way to serve the **raw image**
(so it renders directly in a browser tab or an `<img>` tag) rather than only as a
downloadable attachment.

## Current State (for reference)

- Gallery table: `id, user_id, title, original_filename, stored_path, size_bytes, mime_type, visibility`.
- `GET /gallery` — public, lists `visibility = 'public'` images.
- `GET /gallery/my` — authenticated, lists the current user's images.
- `GET /gallery/{id}` — public, returns one image's metadata.
- `GET /gallery/{id}/download` — public, serves the file with
  `Content-Disposition: attachment`.
- `POST /gallery`, `DELETE /gallery/{id}`, `PATCH .../title`, `PATCH .../visibility`.

## Goal

Give every gallery image a unique 8-character `short_id`, return it in all gallery
responses, and add endpoints to look up and serve an image (metadata + raw bytes)
by its `short_id`, with access rules consistent with the frontend requirements.

## 1. Add `short_id` to the Gallery Schema

1. Add a `short_id CHAR(8) NOT NULL UNIQUE` column to the `gallery` table in the
   `migrate()` function in `src/db.rs`.
2. Follow the existing idempotent migration pattern: check with
   `SHOW COLUMNS FROM gallery LIKE 'short_id'` and `ADD COLUMN` only if missing,
   so existing installations upgrade cleanly.
3. **Backfill** existing rows with a generated unique `short_id` before adding the
   `UNIQUE` constraint (or add the column nullable, backfill, then enforce
   uniqueness). Do not drop the table and do not lose existing rows.
4. Add an index on `short_id` (the `UNIQUE` constraint provides one).

## 2. Generate `short_id` on Upload

1. Add a helper (e.g. in `src/media.rs`) that generates a random 8-character
   string using URL-safe alphanumeric characters (`A–Z`, `a–z`, `0–9`).
2. On upload (`POST /gallery`), generate a `short_id` for each file and insert
   it. Handle the rare uniqueness collision by regenerating and retrying.
3. Include `short_id` in the `GalleryItem` struct so it is returned by every
   gallery response (list, my, get, upload, patch).

## 3. Lookup Endpoint: `GET /gallery/s/{short_id}`

1. Add a route that resolves a `short_id` to a single image's metadata (same shape
   as `GET /gallery/{id}`, now including `short_id`).
2. Return `404 Not Found` if no image matches the `short_id`.
3. Apply access rules (see section 5).

## 4. Raw Image Serving: `GET /gallery/s/{short_id}/raw`

1. Add a route that streams the raw image bytes so it renders inline in a browser
   tab or an `<img src>` — reuse `read_file` from `src/media.rs`.
2. Set `Content-Type` to the stored `mime_type` and use
   `Content-Disposition: inline` (not `attachment`) so the browser displays rather
   than downloads it.
3. Return `404` if the image or the file on disk is missing.
4. Apply access rules (see section 5).

## 5. Access Rules

- The frontend requires that only the **owner** can view their images; a
  `superuser` may view any image. Publicly-visible images (`visibility = 'public'`)
  remain viewable by anyone, consistent with the current public listing.
- Suggested rule for the `s/{short_id}` and `s/{short_id}/raw` endpoints: allow if
  the image is `public`, OR the requester is the owner, OR the requester is a
  `superuser`; otherwise `403 Forbidden`.
- **Browser `<img>` tags cannot send an `Authorization: Bearer` header.** For the
  raw endpoint to work in `<img>` and new-tab navigation, pick and document one
  approach:
  - Treat `public` images as servable without auth via the unguessable `short_id`
    (the 8-char id acts as a capability), and require auth only for `private`
    images; or
  - Accept the JWT via a `?token=` query parameter on the raw endpoint as a
    fallback to the `Authorization` header.
  Document whichever approach is chosen and flag it back to the frontend.

## 6. Documentation

1. Update `api-docs.md`: add `short_id` to all gallery response examples, and
   document `GET /gallery/s/{short_id}` and `GET /gallery/s/{short_id}/raw`
   (including the chosen auth approach for raw serving).
2. Update the endpoint summary table.

## Acceptance Criteria

- The `gallery` table has a unique `short_id CHAR(8)` column; existing rows are
  backfilled and no data is lost.
- Every gallery response includes an 8-character `short_id`.
- New uploads receive a unique `short_id`; collisions are handled gracefully.
- `GET /gallery/s/{short_id}` returns the image metadata, or `404` if unknown.
- `GET /gallery/s/{short_id}/raw` serves the raw image with
  `Content-Disposition: inline` and the correct `Content-Type`, so it renders in a
  browser tab and in `<img>` tags.
- Access rules are enforced: private images are only served to their owner or a
  superuser; unauthorized access returns `403`.
- The chosen strategy for authenticating raw image requests from `<img>`/new-tab
  is implemented and documented.
- `api-docs.md` is updated to reflect the new field and endpoints.
- `cargo check` and `cargo build` run without errors.

## Notes

- This document is high level; implementation details (helper names, exact SQL,
  serving strategy) are left to the implementer, but must follow the existing
  patterns and the conventions in `AGENTS.md`.
- Migrations must stay idempotent and must never drop the `gallery` table or lose
  existing rows.
- Do not use `unwrap()` in production paths; return proper HTTP status codes and
  clear error messages.
- The frontend expects the backend to be running at `http://localhost:3000` for
  end-to-end testing.

