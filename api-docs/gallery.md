# Gallery (Images)

[← Back to Index](README.md)

Gallery endpoints let users upload, list, view, and manage images. All roles can
upload their own images. A `user`/`admin` can only modify their own items; a
`superuser` can modify any item. Files are uploaded with `multipart/form-data`,
saved to disk under `STORAGE_DIR`, and tracked in the database.

Each gallery item stores a `visibility` (`public` or `private`), a unique
8-character `short_id` for shareable URLs, an optional `thumbnail_path`, an
optional `preview_path`, a `pinned` flag, a `pin_order`, and a `status`.

## Upload Constraints (Images)

- Maximum size: **100 MB** (larger uploads are rejected with `413`).
- Allowed extensions: `.jpg`, `.jpeg`, `.png`, `.gif`, `.webp`, `.heic`, `.heif`, `.svg`, `.raw`, `.cr2`, `.nef`, `.dng`
- Bulk upload limit: Up to **50 files** in a single request.

## Background Processing

- Thumbnails and previews are generated **asynchronously in the background** after the upload
  response is returned. The upload endpoint responds with `202 Accepted` as soon as the raw
  files are saved, so the client never waits for image processing.
- Freshly uploaded items start with `status: "processing"` and `thumbnail_path`/`preview_path`
  set to `null`. Once processing completes, `status` becomes `active` and the paths are populated.
- Poll `POST /gallery/status` (or re-fetch the item) to detect when processing has finished.
- A bounded semaphore caps concurrent image decoding to keep memory usage predictable during
  bulk uploads.

## Thumbnails

- Generated in the background using non-blocking image processing (`spawn_blocking`).
- Format: WebP **lossy** (quality 80) for small file size.
- Max width: 500px (height auto-calculated, aspect ratio preserved). Images smaller than 500px are not upscaled.
- Stored alongside the original with a `-thumb.webp` suffix (e.g. `..._UUID-thumb.webp`) and tracked as `thumbnail_path`.
- Served via `GET /gallery/t/{short_id}` (cached for 1 year).
- Thumbnail generation is non-critical — if it fails the item is marked `failed_processing` and can be retried via `POST /gallery/{id}/reprocess`.

## Previews

- Generated in the background in parallel with thumbnails using non-blocking image processing.
  Both are produced from a **single decode** of the source image (the preview is resized first,
  then the thumbnail is derived from the preview) for efficiency.
- Format: WebP **lossy** (quality 85) — a balance between size and quality, higher than thumbnail.
- Max width: 1280px (height auto-calculated, aspect ratio preserved). Images smaller than 1280px are not upscaled.
- Stored alongside the original with a `-preview.webp` suffix (e.g. `..._UUID-preview.webp`) and tracked as `preview_path`.
- Served via `GET /gallery/p/{short_id}` (cached for 1 hour).
- Preview generation is non-critical — if it fails the item is marked `failed_processing` and can be retried.
- **Performance:** Instant loading from disk, no on-the-fly generation delays.

## Pinned Images

- Each item has a `pinned` boolean (defaults to `false`) and a `pin_order` integer (defaults to `0`).
- Pin/unpin via `PATCH /gallery/{id}/pinned` (owner or superuser only).
  - Maximum of **8 pinned images per user**. Attempting to pin a 9th image returns `400 Bad Request`.
  - When pinning: automatically assigned `pin_order = MAX(pin_order) + 1` (sequential order).
  - When unpinning: `pin_order` is reset to `0`.
- List the current user's pinned images via `GET /gallery/me/pinned`.
  - Results are ordered by `pin_order ASC, updated_at DESC` (custom order first, then newest).
- Reorder pinned images via `PATCH /gallery/reorder-pins` to persist drag-and-drop order from frontend.

## Gallery Item Shape

```json
{
  "id": 1,
  "user_id": 1,
  "title": "Sunset",
  "original_filename": "sunset.jpg",
  "stored_path": "gallery/2026/06/2026-06-30/2026-06-30_14-23-05_550e8400-e29b-41d4-a716-446655440000.jpg",
  "size_bytes": 482931,
  "mime_type": "image/jpeg",
  "visibility": "public",
  "short_id": "aB3xYz9Q",
  "thumbnail_path": "gallery/2026/06/2026-06-30/2026-06-30_14-23-05_550e8400-e29b-41d4-a716-446655440000-thumb.webp",
  "preview_path": "gallery/2026/06/2026-06-30/2026-06-30_14-23-05_550e8400-e29b-41d4-a716-446655440000-preview.webp",
  "pinned": false,
  "status": "active",
  "pin_order": 0
}
```
`thumbnail_path` and `preview_path` are omitted when generation failed or is still pending.
`pin_order` is `0` for unpinned images, and `1-8` for pinned images based on user's custom order.

## Status Field

- `processing` — the raw file is saved and thumbnail/preview generation is running in the
  background. This is the initial state of every freshly uploaded item.
- `active` — thumbnail and preview generation finished successfully; `thumbnail_path` and
  `preview_path` are populated.
- `failed_processing` — thumbnail/preview generation failed; the raw file is safe and can be
  retried via `POST /gallery/{id}/reprocess`.

---

## GET /gallery/public

Lists all **public** images with **cursor-based pagination**, newest first. Public endpoint.

**Query Parameters:**
- `cursor` (optional): The `id` of the last item from the previous page. Omit on the first request.
- `limit` (optional): Number of items per page. Defaults to `50`. Maximum `100`.

**Example:** `GET /gallery/public?cursor=1450&limit=20`

**Pagination behavior:**
- Results are ordered by `id DESC` (newest first).
- The first request (no `cursor`) returns the first page.
- Each response includes `next_cursor` — use it as `?cursor={next_cursor}` for the next page.
- When `next_cursor` is `null`, you've reached the end.
- Ideal for infinite-scroll masonry grids on the frontend.

Response `200`:
```json
{
  "success": true,
  "data": {
    "items": [
      {
        "id": 1,
        "user_id": 1,
        "title": "Sunset",
        "original_filename": "sunset.jpg",
        "stored_path": "gallery/2026/06/2026-06-30/2026-06-30_14-23-05_550e8400-e29b-41d4-a716-446655440000.jpg",
        "size_bytes": 482931,
        "mime_type": "image/jpeg",
        "visibility": "public",
        "short_id": "aB3xYz9Q",
        "thumbnail_path": "gallery/2026/06/2026-06-30/2026-06-30_14-23-05_550e8400-e29b-41d4-a716-446655440000-thumb.webp",
        "preview_path": "gallery/2026/06/2026-06-30/2026-06-30_14-23-05_550e8400-e29b-41d4-a716-446655440000-preview.webp",
        "pinned": false,
        "status": "active",
        "pin_order": 0
      }
    ],
    "next_cursor": 450,
    "limit": 50
  }
}
```

**Notes:**
- `next_cursor` is `null` when there are no more items.
- `limit` is clamped to a maximum of 100 to protect server performance.

```bash
# First page
curl http://localhost:3000/gallery/public?limit=20

# Next page
curl "http://localhost:3000/gallery/public?cursor=450&limit=20"
```

## GET /gallery/me

Lists the current user's images (both public and private) with **cursor-based pagination**, newest first. **Requires authentication.**

**Query Parameters:**
- `cursor` (optional): The `id` of the last item from the previous page. Omit on the first request.
- `limit` (optional): Number of items per page. Defaults to `50`. Maximum `100`.

**Example:** `GET /gallery/me?cursor=1450&limit=20`

Response `200`: Same pagination envelope as `GET /gallery/public`, but scoped to the authenticated user's images only.

```json
{
  "success": true,
  "data": {
    "items": [ /* user's GalleryItem[] */ ],
    "next_cursor": 320,
    "limit": 50
  }
}
```

```bash
curl "http://localhost:3000/gallery/me?limit=20" \
  -H "Authorization: Bearer <token>"
```

## GET /gallery/me/pinned

Lists the current user's **pinned** images, ordered by `pin_order ASC, updated_at DESC` (i.e. user's saved order, with newly pinned items at the end). **Requires authentication.**

**This endpoint does NOT use pagination** — pinned images are limited to 8 per user, so the response is always small.

Response `200`: array of gallery items where `pinned` is `true`, ordered by `pin_order`.

```bash
curl http://localhost:3000/gallery/me/pinned \
  -H "Authorization: Bearer <token>"
```

## POST /gallery/status

Checks the processing `status` of multiple images in a single request. **Requires authentication.**
Intended for polling after an upload while background processing runs. Only the authenticated
user's own images are considered.

Request body:
```json
{ "ids": [1, 2, 3] }
```

- `ids` — array of numeric gallery ids. Must be non-empty; up to **100** ids per request.

Response `200` — an object mapping each found id to its current status
(`processing`, `active`, or `failed_processing`):
```json
{
  "success": true,
  "data": {
    "1": "active",
    "2": "processing",
    "3": "failed_processing"
  }
}
```

Ids that don't exist or aren't owned by the current user are simply omitted from the map; the
client can treat missing ids as not-found.

Errors:
- `400` — `ids` is empty, or contains more than 100 entries.

```bash
curl -X POST http://localhost:3000/gallery/status \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"ids":[1,2,3]}'
```

## POST /gallery

Uploads image file(s) for the current user. **Requires authentication.** Allowed for all roles.
Uses `multipart/form-data`:

| Field | Required | Description |
|-------|----------|-------------|
| `file` | Yes | The image file(s). Send multiple `file` fields for bulk upload (max 50). |
| `title` | No | Display title. Applied only when a single file is uploaded; otherwise the original filename is used. |
| `visibility` | No | `public` or `private`. Defaults to `private`. |

**Upload process:**
1. Validates each file's extension and size (max 100 MB).
2. Saves the original file(s) to disk.
3. Generates a unique `short_id` and inserts metadata into the database with `status: "processing"`
   inside a transaction.
4. Returns `202 Accepted` immediately — the client does **not** wait for image processing.
5. In the background: generates the preview (WebP lossy quality 85, max 1280px) and thumbnail
   (WebP lossy quality 80, max 500px) from a single decode, saves them to disk, and updates the
   item to `status: "active"` with `thumbnail_path`/`preview_path` populated. On failure the item
   becomes `status: "failed_processing"`.

Newly uploaded items always start with `pinned: false`, `pin_order: 0`, `status: "processing"`, and
`thumbnail_path`/`preview_path` as `null`. Use `POST /gallery/status` to poll for completion.

Response `202` (single file) — envelope `data` is a single item:
```json
{
  "success": true,
  "data": {
    "id": 1,
    "user_id": 1,
    "title": "Sunset",
    "original_filename": "sunset.jpg",
    "stored_path": "gallery/2026/06/2026-06-30/2026-06-30_14-23-05_550e8400-e29b-41d4-a716-446655440000.jpg",
    "size_bytes": 482931,
    "mime_type": "image/jpeg",
    "visibility": "private",
    "short_id": "aB3xYz9Q",
    "pinned": false,
    "status": "processing",
    "pin_order": 0
  }
}
```
Note: `thumbnail_path` and `preview_path` are omitted (they are `null`) until background
processing finishes.

Response `202` (bulk upload) — envelope `data` is an array of items, each with `status: "processing"`.

Errors:
- `400` — no file provided, missing filename, unsupported extension, or more than 50 files.
- `413` — a file exceeds 100 MB.

```bash
curl -X POST http://localhost:3000/gallery \
  -H "Authorization: Bearer <token>" \
  -F "title=Sunset" \
  -F "visibility=public" \
  -F "file=@sunset.jpg"
```

## GET /gallery/{id}

Returns a single image's metadata by numeric id (same shape as a list item). Public endpoint.

Errors:
- `404` — image not found.

## GET /gallery/d/{id}

Downloads the raw image file with `Content-Disposition: attachment` (forces a download).
The response uses the stored `Content-Type` and original filename. Public endpoint.

Errors:
- `404` — image not found, or file missing on disk.

```bash
curl -O http://localhost:3000/gallery/d/1
```

## GET /gallery/r/{short_id}

Serves the raw full-size image inline (`Content-Disposition: inline`) so it renders in a
browser tab or `<img>` tag. This is the endpoint for shareable image URLs.

**Access rules:**
- **Public images**: accessible to anyone via the unguessable `short_id` — no auth required.
- **Private images**: require one of the following (checked in this order):
  1. **Signed URL**: query params `?expires={unix_ts}&sig={signature}` (see `POST /gallery/{short_id}/sign`).
  2. **Cookie**: the `auth_token` cookie.
  3. **Header**: `Authorization: Bearer <token>`.
  The authenticated user must be the owner or a `superuser`.

**Error responses use content negotiation** (based on the `Accept` header):
- Browser requests (`Accept: text/html`) receive a styled HTML error page with a "Back to Home" link.
- API clients receive the standard JSON error envelope.
- Status codes are the same for both: `401`, `403`, `404`.

Errors:
- `401` — image is private and no valid auth/signed URL was provided (or the signed URL expired/invalid).
- `403` — authenticated but not the owner or a superuser.
- `404` — image not found, or file missing on disk.

```bash
# Public image in an <img> tag (no auth)
<img src="http://localhost:3000/gallery/r/aB3xYz9Q" alt="Sunset" />

# Private image with a signed URL
<img src="http://localhost:3000/gallery/r/xYz9QaB3?expires=1719936000&sig=a1b2c3d4e5f67890" alt="Private" />

# Private image via fetch (cookie sent automatically)
fetch('http://localhost:3000/gallery/r/xYz9QaB3', { credentials: 'include' })
```

## GET /gallery/t/{short_id}

Serves the **pre-generated thumbnail** inline (WebP, lossy quality 80, max width 500px).
Optimized for lists and grids; cached for 1 year (`Cache-Control: public, max-age=31536000`).

Access rules and error handling are identical to `GET /gallery/r/{short_id}`.
Falls back to serving the original image if the thumbnail is missing (backward compatibility).

Errors:
- `401` / `403` — same as the raw endpoint for private images.
- `404` — image not found, or the thumbnail file is missing on disk.

```bash
<img src="http://localhost:3000/gallery/t/aB3xYz9Q" alt="Sunset thumbnail" />
```

## GET /gallery/p/{short_id}

Serves a **pre-generated preview** (medium-size) image inline. Generated by background processing
after upload and stored on disk (format: WebP lossy quality 85, max width 1280px). Fast loading,
no on-the-fly generation. Cached for 1 hour (`Cache-Control: public, max-age=3600`).

Access rules and error handling are identical to `GET /gallery/r/{short_id}` and `GET /gallery/t/{short_id}`.

Falls back to serving the original image if the preview is missing — e.g. while the item is still
`processing`, or for older items / after a failed processing (backward compatibility).

Errors:
- `401` / `403` — same as the raw endpoint for private images.
- `404` — image not found, or file missing on disk.

```bash
<img src="http://localhost:3000/gallery/p/aB3xYz9Q" alt="Preview" />
```

## POST /gallery/{short_id}/sign

Generates a **signed URL** for a private image, so it can be embedded in an `<img>` tag
without exposing a JWT. **Requires authentication.** Owner or superuser only.

- The signature is derived from `short_id`, `user_id`, `expires`, and the server secret.
- Signed URLs are valid for **15 minutes**.
- The returned signature also works for the `/t/{short_id}` (thumbnail) and `/p/{short_id}` (preview)
  endpoints — reuse the same `expires` and `sig` query params.

Response `200`:
```json
{
  "success": true,
  "data": {
    "url": "http://localhost:3000/gallery/r/xYz9QaB3?expires=1719936000&sig=a1b2c3d4e5f67890",
    "expires_at": 1719936000
  }
}
```

Errors:
- `403` — not the owner and not a superuser.
- `404` — image not found.

```bash
curl -X POST http://localhost:3000/gallery/xYz9QaB3/sign \
  -H "Authorization: Bearer <token>"
```

## PATCH /gallery/{id}

Unified partial update endpoint for gallery images. Updates any combination of **title**, **visibility**, and **pinned** status in a single request. **Requires authentication.** Owner or superuser only.

**All fields are optional** — only send the fields you want to change.

Request body (all fields optional):
```json
{
  "title": "Beautiful Sunset",
  "visibility": "public",
  "pinned": true
}
```

Or update just one field:
```json
{ "title": "New Title" }
```

**Field validation rules:**
- `title`: Must not be empty or whitespace. If provided → `400` if empty.
- `visibility`: Must be `public` or `private` (case-insensitive). If provided → `400` if invalid.
- `pinned`: Applies the following logic:
  - **Pinning** (when `pinned: true` and image is not already pinned):
    - Checks if user already has 8 pinned images. If so → `400`.
    - Assigns `pin_order = MAX(current_pin_order) + 1` (appends to end of pinned list).
  - **Unpinning** (when `pinned: false` and image is pinned):
    - Sets `pin_order = 0` (removes from ordered list).
  - No change if `pinned` value matches current state.

Response `200`: the updated gallery item with all changes applied.

```json
{
  "success": true,
  "data": {
    "id": 1,
    "title": "Beautiful Sunset",
    "visibility": "public",
    "pinned": true,
    "pin_order": 3,
    ...
  }
}
```

Errors:
- `400` — no fields provided, or validation failed (empty title, invalid visibility, max pins exceeded).
- `403` — not the owner and not a superuser.
- `404` — image not found.

**Examples:**

```bash
# Update title and visibility
curl -X PATCH http://localhost:3000/gallery/1 \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"title":"New Title","visibility":"public"}'

# Pin an image
curl -X PATCH http://localhost:3000/gallery/1 \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"pinned":true}'

# Update all three fields at once
curl -X PATCH http://localhost:3000/gallery/1 \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"title":"Updated","visibility":"private","pinned":false}'
```

## PATCH /gallery/reorder-pins

Persists a new order for the current user's pinned images. Used to save drag-and-drop reordering from the frontend. **Requires authentication.** Owner or superuser only.

**Note:** This endpoint is separate from `PATCH /gallery/{id}` because it operates across multiple images transactionally.

Request body:
```json
{
  "ordered_ids": [5, 2, 8, 1]
}
```

- `ordered_ids` — array of image IDs in the desired order. Must be non-empty, max 8 IDs.
- All IDs must:
  - Exist and belong to the current user (or user must be superuser).
  - Already be pinned (`pinned = true`).

**Behavior:**
- Validates all IDs (ownership + pinned status) in a transaction.
- Updates `pin_order` for each ID based on array position (1st ID → `pin_order = 1`, 2nd ID → `pin_order = 2`, etc.).
- Changes are atomic (all-or-nothing).

Response `200`:
```json
{ "success": true, "data": "Pin order updated successfully" }
```

Errors:
- `400` — `ordered_ids` is empty, contains more than 8 IDs, or references an unpinned image.
- `403` — one or more IDs belong to another user (and current user is not superuser).
- `404` — one or more IDs don't exist.

```bash
curl -X PATCH http://localhost:3000/gallery/reorder-pins \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"ordered_ids": [5, 2, 8, 1]}'
```

## DELETE /gallery/{id}

Deletes an image — removes the database record, the original file, the thumbnail, and the
preview (whichever are present). **Requires authentication.** Owner or superuser only.

Response `200`:
```json
{ "success": true, "data": "Image deleted" }
```

Errors:
- `403` — not the owner and not a superuser.
- `404` — image not found.

```bash
curl -X DELETE http://localhost:3000/gallery/1 \
  -H "Authorization: Bearer <token>"
```

## POST /gallery/{id}/reprocess

Retries thumbnail **and preview** generation for an image. Useful when an upload's background
processing failed (status = `failed_processing`). **Requires authentication.** Owner or superuser only.

Reprocessing uses the **same async background pattern as upload** — returns `202 Accepted` immediately,
then processes in the background.

**Process:**
1. Verifies the image exists and the user has permission (synchronous checks before queuing).
2. Confirms the original raw file exists on disk (must exist).
3. Sets status to `processing`.
4. **Returns `202 Accepted` immediately** with the item in `processing` status.
5. In the background: acquires a semaphore permit, generates thumbnail and preview from a single decode (non-blocking), saves both files in parallel, and updates status to `active` (or `failed_processing` on error).

**Client workflow:**
- Poll `POST /gallery/status` with the image `id` to check when reprocessing finishes.
- When status becomes `active`, the thumbnail and preview are ready.
- If status becomes `failed_processing`, reprocessing failed again.

Response `202 Accepted`: the gallery item with `status: "processing"`. Background processing continues.

```json
{
  "success": true,
  "data": {
    "id": 1,
    "user_id": 1,
    "title": "Sunset",
    "original_filename": "sunset.jpg",
    "stored_path": "gallery/2026/06/2026-06-30/2026-06-30_14-23-05_550e8400-e29b-41d4-a716-446655440000.jpg",
    "size_bytes": 482931,
    "mime_type": "image/jpeg",
    "visibility": "public",
    "short_id": "aB3xYz9Q",
    "pinned": false,
    "status": "processing",
    "pin_order": 0
  }
}
```

Note: `thumbnail_path` and `preview_path` are omitted (null) until background processing completes.

Errors (returned synchronously before queuing):
- `403` — not the owner and not a superuser.
- `404` — image not found, or raw file missing from disk.

```bash
curl -X POST http://localhost:3000/gallery/1/reprocess \
  -H "Authorization: Bearer <token>"

# Then poll status
curl -X POST http://localhost:3000/gallery/status \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"ids":[1]}'
```
