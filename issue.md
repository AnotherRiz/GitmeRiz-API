# Planning: Friendly HTML Error Pages for Direct Image Access

This document describes the **backend** work for Task 2 of GitmeRiz Web issue
[#16 "Improve Image Modal & Private Image Access Handling"](https://github.com/AnotherRiz/GitmeRiz/issues/16).

The frontend portion of Task 2 (fetch-based loading, in-modal `401`/`403` UI) is
handled by the web app. This issue covers the piece explicitly flagged as a backend
change in section **2.3 — Direct backend URL access**.

## Background & Dependency

When a browser opens an image URL directly (e.g. pasting
`http://localhost:3000/gallery/r/knkJbMUv` into the address bar, or "Open image in
new tab"), the request is served by this backend, **not** the React app. Today the
backend always answers with raw JSON, so the user sees:

```json
{ "success": false, "error": "This image is private. Authentication required." }
```

The frontend team asked (in the PR notes for #16) that the backend return a friendly
**HTML error page** when the client is a browser, while continuing to return **JSON**
for programmatic clients (the modal's `fetch`, API consumers, `curl`).

## Current State (for reference)

In `src/handlers/gallery.rs`, the image-serving handlers return JSON errors via
`Json(ApiResponse::<()>::error(...))` with the proper status code:

- `serve_raw_image` — `GET /gallery/r/{short_id}`
- `serve_thumbnail_image` — `GET /gallery/t/{short_id}`

Error branches currently produce:
- `401 Unauthorized` — private image, no/invalid auth, or expired/invalid signed URL.
- `403 Forbidden` — authenticated but not the owner/superuser.
- `404 Not Found` — unknown `short_id`, or file missing on disk.

All other endpoints (metadata, uploads, users, etc.) are API/JSON only and are **out
of scope**.

## Goal

For the browser-facing image endpoints, choose the error response format based on the
request's `Accept` header (content negotiation):

- Request prefers `text/html` (typical browser navigation) → return a small, styled
  **HTML error page** with the matching status code and a **"Back to Home"** button.
- Otherwise (JSON/`fetch`/`curl`/API clients) → keep the **current JSON** behavior
  unchanged.

Successful image responses (the actual image bytes) are unaffected.

## 1. Content Negotiation Helper

1. Add a helper (e.g. in `src/media.rs` or a new small `src/handlers/error_page.rs`)
   that decides the response format from the request headers:
   - Read the `Accept` header. Treat the request as "browser/HTML" when `Accept`
     contains `text/html`. Everything else (including missing `Accept` and
     `application/json`) is treated as "API/JSON".
2. Add a helper that builds an error `Response` given a `StatusCode` and a
   user-facing message:
   - **HTML branch**: body is an HTML document, `Content-Type: text/html; charset=utf-8`,
     same status code.
   - **JSON branch**: body is `Json(ApiResponse::<()>::error(message))`, same status
     code — identical to today's output.
3. Keep the helper self-contained and reusable so both `serve_raw_image` and
   `serve_thumbnail_image` share the exact same logic. Do not use `unwrap()` in
   production paths (build the `Response` safely, matching existing style).

## 2. Wire the Handlers to the Helper

1. `serve_raw_image` and `serve_thumbnail_image` need access to the request's
   `Accept` header. Add a `headers: HeaderMap` extractor (already imported/used in
   these handlers) and pass it to the helper — no change to the auth/serving logic.
2. Replace each error return in these two handlers with a call to the shared helper,
   preserving the existing status codes and messages:
   - `401` — "This image is private. Authentication required." / signed-URL error text.
   - `403` — "You can only access your own private images".
   - `404` — "Image not found" / "File not found on disk" / "Thumbnail not found on disk".
3. Do **not** change the success paths (image bytes, `Content-Type`,
   `Content-Disposition`, cache headers).

## 3. HTML Error Page Content

1. A single minimal, self-contained HTML template (inline CSS, no external assets)
   that renders:
   - A large heading: `{status} | Unauthorized access to this image.` for `401`/`403`.
   - For `404`, a suitable message such as `404 | Image not found.`
   - A **"Back to Home"** link/button pointing at the frontend app root.
2. The message wording for `401`/`403` should match the frontend copy in issue #16
   ("Unauthorized access to this image.") for a consistent experience.
3. The page should be readable standalone (dark background, centered content) and must
   not reference the React app's assets.

## 4. Frontend "Home" URL (Configuration)

1. The "Back to Home" button must point to the **web app**, not the backend. Add a
   config value for this, e.g. `FRONTEND_URL`, loaded in `src/config.rs` from the
   `.env` (following the existing config-loading pattern), defaulting to
   `http://localhost:5173` for development.
2. Use forward slashes in the `.env` value. Update `.env.example` and document the new
   variable (per `AGENTS.md` environment-variable rules).

## 5. Documentation

1. Update `api-docs.md` for `GET /gallery/r/{short_id}` and `GET /gallery/t/{short_id}`:
   note that error responses are **content-negotiated** — HTML for `Accept: text/html`,
   JSON otherwise — and that status codes are unchanged.
2. Note the new `FRONTEND_URL` environment variable in the environment section of
   `README.md` / `api-docs.md` as appropriate.

## Access Rules (unchanged)

- Public images: served to anyone via the unguessable `short_id`.
- Private images: owner or superuser only (cookie, header, or valid signed URL).
- This issue only changes the **format** of error responses for browser requests; it
  does not change **who** can access an image.

## Acceptance Criteria

- Opening a private image URL directly in a browser tab (no/instufficient access)
  shows a styled HTML page reading `401 | Unauthorized access to this image.` or
  `403 | Unauthorized access to this image.`, with a working "Back to Home" button
  that navigates to the frontend app.
- Opening an unknown `short_id` in a browser shows a styled `404` HTML page.
- Programmatic clients are unaffected: `fetch` (the modal), `curl`, and any request
  without `Accept: text/html` still receive the existing JSON envelope with the same
  status codes.
- Successful image requests still return the raw image bytes with correct
  `Content-Type` — no regression.
- Both `serve_raw_image` and `serve_thumbnail_image` use the same shared helper (no
  duplicated HTML/branching logic).
- `FRONTEND_URL` is configurable via `.env`, documented, and defaults sensibly.
- `cargo check` and `cargo build` run without errors; no `unwrap()` in production paths.

## Out of Scope

- The frontend modal work in issue #16 (Task 1 and Task 2.1/2.2) — handled in the web repo.
- `GET /gallery/d/{id}` (force-download) and metadata endpoints — remain JSON only.
- Any change to authentication, signed-URL logic, or access rules.

## Notes

- This document is high level; exact helper names, template markup, and file placement
  are left to the implementer but must follow the existing patterns in
  `src/handlers/gallery.rs`, `src/media.rs`, `src/config.rs`, and the conventions in
  `AGENTS.md`.
- Keep the HTML tiny and dependency-free (inline styles); this is an error fallback,
  not a full page framework.
