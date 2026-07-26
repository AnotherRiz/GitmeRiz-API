# Audio

[← Back to Index](README.md)

All audio endpoints support optional authentication via cookies or `Authorization: Bearer <token>` header.
Audio items have `visibility` fields (`public` or `private`). Public items are viewable/downloadable by anyone;
private items require authentication and ownership. Upload and deletion require authentication.

**Upload constraints (audio):**
- No size limit.
- Allowed extensions: `.mp3`, `.m4a`, `.aac`, `.ogg`, `.wav`, `.flac`
- `.aac` files are automatically remuxed to `.m4a` (lossless container wrap, not re-encoded).
- Supported fields: `title` (optional), `description` (optional), `visibility` (optional, defaults to `private`),
  `thumbnail` (optional cover art image).

**Thumbnail (cover art):**
- Fully optional — omit the field entirely, or send an empty value, to skip it.
- Allowed image extensions: `.jpg`, `.jpeg`, `.png`, `.webp`, `.gif`.
- Resized to a WebP thumbnail (max 500px width, quality 80, never upscaled).
- If thumbnail processing fails for any reason, the audio upload still succeeds without a thumbnail
  (non-fatal — check `thumbnail_path` in the response to see if one was generated).
- Served inline via `GET /audio/{id}/thumbnail` (same visibility access rules as the audio item itself).

**Format handling:**
| Extension | Behavior |
|-----------|----------|
| `.mp3` | Serve as-is (no processing) |
| `.m4a` | Serve as-is (already AAC-in-MP4 container) |
| `.aac` | Remux to `.m4a` (lossless container wrap via FFmpeg `-c:a copy`) |
| `.ogg` | Serve as-is |
| `.wav` | Serve as-is |
| `.flac` | Serve as-is |

## GET /audio/public

Lists all public audio items (no authentication required).

Response `200`:
```json
{
  "success": true,
  "data": [
    {
      "id": 1,
      "user_id": 1,
      "title": "Public Song",
      "description": "A great public song",
      "original_filename": "song.mp3",
      "stored_path": "audio/2026/07/2026-07-22/2026-07-22_14-26-40_3fa85f64-5717-4562-b3fc-2c963f66afa6.mp3",
      "size_bytes": 5242880,
      "mime_type": "audio/mpeg",
      "visibility": "public",
      "thumbnail_path": "audio/2026/07/2026-07-22/2026-07-22_14-26-40_3fa85f64-5717-4562-b3fc-2c963f66afa6-thumb.webp",
      "pinned": false,
      "pin_order": 0,
      "short_id": "AbC12XyZ",
      "created_at": "2026-07-22T14:26:40Z"
    }
  ]
}
```

## GET /audio

Lists audio for the authenticated user. `superuser` sees all, others see only their own.
Requires authentication.

Response `200`:
```json
{
  "success": true,
  "data": [
    {
      "id": 2,
      "user_id": 1,
      "title": "Private Song",
      "description": "My private recording",
      "original_filename": "private.mp3",
      "stored_path": "audio/2026/07/2026-07-22/2026-07-22_14-26-40_550e8400-e29b-41d4-a716-446655440000.mp3",
      "size_bytes": 3145728,
      "mime_type": "audio/mpeg",
      "visibility": "private",
      "thumbnail_path": null,
      "pinned": false,
      "pin_order": 0,
      "short_id": "DeF45GhI",
      "created_at": "2026-07-22T14:26:40Z"
    }
  ]
}
```

## POST /audio

Uploads an audio file for the current user (requires authentication).
`multipart/form-data` fields:
- `file` (required) — the audio file.
- `title` (optional) — audio title; defaults to filename if omitted.
- `description` (optional) — description text; omitted if empty.
- `visibility` (optional) — `public` or `private`; defaults to `private`.
- `thumbnail` (optional) — cover art image (`.jpg`, `.jpeg`, `.png`, `.webp`, `.gif`); omit or leave empty to skip.

Response `201`:
```json
{
  "success": true,
  "data": {
    "id": 3,
    "user_id": 1,
    "title": "My Recording",
    "description": "A recording session",
    "original_filename": "recording.aac",
    "stored_path": "audio/2026/07/2026-07-22/2026-07-22_14-26-40_a1b2c3d4-e5f6-7890-abcd-ef1234567890.m4a",
    "size_bytes": 2097152,
    "mime_type": "audio/mp4",
    "visibility": "private",
    "thumbnail_path": "audio/2026/07/2026-07-22/2026-07-22_14-26-40_a1b2c3d4-e5f6-7890-abcd-ef1234567890-thumb.webp",
    "pinned": false,
    "pin_order": 0,
    "short_id": "GhI67JkL",
    "created_at": "2026-07-22T14:26:40Z"
  }
}
```

**Notes:**
- For `.aac` uploads, `original_filename` will still end in `.aac`, but `stored_path` will end in `.m4a` (remuxed).
- `size_bytes` reflects the originally uploaded byte count, not the remuxed output.
- `mime_type` is derived from the final stored extension (`.m4a` for remuxed files).
- `thumbnail_path` is `null` when no thumbnail was provided or if thumbnail processing failed.

Errors:
- `400` — no file provided, missing filename, unsupported extension, or invalid visibility value.

```bash
# Upload with description, public visibility, and a cover art thumbnail
curl -X POST http://localhost:3000/api/audio \
  -H "Authorization: Bearer <token>" \
  -F "title=My Song" \
  -F "description=A great song" \
  -F "visibility=public" \
  -F "file=@song.mp3" \
  -F "thumbnail=@cover.jpg"

# Upload without description or thumbnail (both optional)
curl -X POST http://localhost:3000/api/audio \
  -H "Authorization: Bearer <token>" \
  -F "title=My Song" \
  -F "file=@song.mp3"

# Upload a raw AAC file (will be remuxed to M4A)
curl -X POST http://localhost:3000/api/audio \
  -H "Authorization: Bearer <token>" \
  -F "title=Raw AAC" \
  -F "file=@audio.aac"
```

## GET /audio/{id}

Returns a single audio item's metadata (public endpoint with visibility check).

**Access rules:**
- `public` items: no authentication required.
- `private` items: requires authentication and ownership (or `superuser`).

Response `200`:
```json
{
  "success": true,
  "data": {
    "id": 1,
    "user_id": 1,
    "title": "Public Song",
    "description": null,
    "original_filename": "song.mp3",
    "stored_path": "audio/2026/07/2026-07-22/2026-07-22_14-26-40_3fa85f64-5717-4562-b3fc-2c963f66afa6.mp3",
    "size_bytes": 5242880,
    "mime_type": "audio/mpeg",
    "visibility": "public",
    "thumbnail_path": null,
    "pinned": false,
    "pin_order": 0,
    "short_id": "AbC12XyZ",
    "created_at": "2026-07-22T14:26:40Z"
  }
}
```

Errors:
- `401` — private audio and no authentication provided.
- `403` — private audio and authenticated user is not the owner (and not `superuser`).
- `404` — audio not found.

## GET /audio/{id}/thumbnail

Serves the cover art thumbnail image inline (WebP, cached 1 year). Public endpoint with visibility check.

**Access rules:**
- `public` items: no authentication required.
- `private` items: requires authentication and ownership (or `superuser`).

Errors:
- `401` — private audio and no authentication provided.
- `403` — private audio and authenticated user is not the owner (and not `superuser`).
- `404` — audio not found, has no thumbnail, or thumbnail file missing on disk.

```bash
curl -o cover.webp http://localhost:3000/api/audio/1/thumbnail
```

## GET /audio/{id}/download

Downloads the actual audio file (public endpoint with visibility check).

**Access rules:**
- `public` items: no authentication required.
- `private` items: requires authentication and ownership (or `superuser`).

Returns the audio file with `Content-Disposition: attachment; filename="..."` header.

Errors:
- `401` — private audio and no authentication provided.
- `403` — private audio and authenticated user is not the owner (and not `superuser`).
- `404` — audio not found or file missing on disk.

```bash
# Download a public audio file
curl -o song.mp3 http://localhost:3000/api/audio/1/download

# Download a private audio file (with auth)
curl -o song.mp3 \
  -H "Authorization: Bearer <token>" \

## PATCH /audio/{id}

Updates audio metadata (title, description, visibility, or pinned status). Supports partial updates;
at least one field must be provided. Owner or `superuser` only. Requires authentication.

**Request body** (all fields optional, at least one required):
```json
{
  "title": "New Title (optional)",
  "description": "New description (optional; empty string clears it)",
  "visibility": "public or private (optional)",
  "pinned": true or false (optional)
}
```

**Pinning constraints:**
- Maximum **8 pinned audio items per user**.
- When pinning: assigns `pin_order = MAX(pin_order) + 1` for the current user.
- When unpinning: resets `pin_order = 0`.
- Pinning the 9th item returns `400 Bad Request`.

Response `200`:
```json
{
  "success": true,
  "data": {
    "id": 1,
    "user_id": 1,
    "title": "New Title",
    "description": "New description",
    "original_filename": "song.mp3",
    "stored_path": "audio/2026/07/2026-07-22/2026-07-22_14-26-40_3fa85f64-5717-4562-b3fc-2c963f66afa6.mp3",
    "size_bytes": 5242880,
    "mime_type": "audio/mpeg",
    "visibility": "public",
    "thumbnail_path": null,
    "pinned": true,
    "pin_order": 3,
    "short_id": "AbC12XyZ",
    "created_at": "2026-07-22T14:26:40Z"
  }
}
```

Errors:
- `400` — all fields are `null`/missing, title becomes empty after trim, invalid visibility value, or attempted to pin 9th item.
- `403` — authenticated user does not own the audio.
- `404` — audio not found.

```bash
# Pin an audio item
curl -X PATCH http://localhost:3000/api/audio/1 \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"pinned": true}'

# Update title and pin simultaneously
curl -X PATCH http://localhost:3000/api/audio/1 \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"title": "Updated Title", "pinned": true}'

# Change visibility to public
curl -X PATCH http://localhost:3000/api/audio/1 \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"visibility": "public"}'
```

## GET /audio/me/pinned

Lists the current user's pinned audio items, ordered by `pin_order` ascending, then `updated_at` descending.
Requires authentication.

Response `200`:
```json
{
  "success": true,
  "data": [
    {
      "id": 1,
      "user_id": 1,
      "title": "Favorite Song",
      "description": null,
      "original_filename": "song.mp3",
      "stored_path": "audio/2026/07/2026-07-22/2026-07-22_14-26-40_3fa85f64-5717-4562-b3fc-2c963f66afa6.mp3",
      "size_bytes": 5242880,
      "mime_type": "audio/mpeg",
      "visibility": "private",
      "thumbnail_path": null,
      "pinned": true,
      "pin_order": 1,
      "short_id": "AbC12XyZ",
      "created_at": "2026-07-22T14:26:40Z"
    },
    {
      "id": 2,
      "user_id": 1,
      "title": "Another Favorite",
      "description": "Good recording",
      "original_filename": "other.mp3",
      "stored_path": "audio/2026/07/2026-07-22/2026-07-22_14-26-45_550e8400-e29b-41d4-a716-446655440000.mp3",
      "size_bytes": 3145728,
      "mime_type": "audio/mpeg",
      "visibility": "public",
      "thumbnail_path": null,
      "pinned": true,
      "pin_order": 2,
      "short_id": "DeF45GhI",
      "created_at": "2026-07-22T14:26:45Z"
    }
  ]
}
```

```bash
curl http://localhost:3000/api/audio/me/pinned \
  -H "Authorization: Bearer <token>"
```

## PATCH /audio/reorder-pins

Reorders the current user's pinned audio items via drag-and-drop. Validates that all items are
owned, pinned, and within the 8-item limit. Transactional — all updates succeed or all fail.
Requires authentication.

**Request body:**
```json
{
  "ordered_ids": [2, 1, 3, 4, 5, 6, 7, 8]
}
```

Response `200`:
```json
{
  "success": true,
  "data": "Pins reordered successfully"
}
```

Errors:
- `400` — `ordered_ids` is empty or exceeds 8 items, or an item is not pinned.
- `403` — authenticated user does not own an item (and is not `superuser`).
- `404` — an item not found.
- `500` — database transaction error.

```bash
curl -X PATCH http://localhost:3000/api/audio/reorder-pins \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"ordered_ids": [3, 1, 2]}'
```

## GET /audio/info/{short_id}

Returns a single audio item's metadata by `short_id` (public endpoint with visibility check).
Same behavior as `GET /audio/{id}`, but uses the 8-character `short_id` instead of numeric `id`.

**Access rules:**
- `public` items: no authentication required.
- `private` items: requires authentication and ownership (or `superuser`).

Response `200`:
```json
{
  "success": true,
  "data": {
    "id": 1,
    "user_id": 1,
    "title": "Public Song",
    "description": null,
    "original_filename": "song.mp3",
    "stored_path": "audio/2026/07/2026-07-22/2026-07-22_14-26-40_3fa85f64-5717-4562-b3fc-2c963f66afa6.mp3",
    "size_bytes": 5242880,
    "mime_type": "audio/mpeg",
    "visibility": "public",
    "thumbnail_path": null,
    "pinned": false,
    "pin_order": 0,
    "short_id": "AbC12XyZ",
    "created_at": "2026-07-22T14:26:40Z"
  }
}
```

Errors:
- `401` — private audio and no authentication provided.
- `403` — private audio and authenticated user is not the owner (and not `superuser`).
- `404` — audio not found.

```bash
curl http://localhost:3000/api/audio/info/AbC12XyZ
```

## GET /audio/download/{short_id}

Downloads the actual audio file by `short_id` (public endpoint with visibility check).
Same behavior as `GET /audio/{id}/download`, but uses the 8-character `short_id` instead of numeric `id`.

**Access rules:**
- `public` items: no authentication required.
- `private` items: requires authentication and ownership (or `superuser`).

Returns the audio file with `Content-Disposition: attachment; filename="..."` header.

Errors:
- `401` — private audio and no authentication provided.
- `403` — private audio and authenticated user is not the owner (and not `superuser`).
- `404` — audio not found or file missing on disk.

```bash
# Download a public audio file by short_id
curl -o song.mp3 http://localhost:3000/api/audio/download/AbC12XyZ

# Download a private audio file by short_id (with auth)
curl -o song.mp3 \
  -H "Authorization: Bearer <token>" \
  http://localhost:3000/api/audio/download/DeF45GhI
```

## GET /audio/thumb/{short_id}

Serves the cover art thumbnail image inline (WebP, cached 1 year) by `short_id`.
Same behavior as `GET /audio/{id}/thumbnail`, but uses the 8-character `short_id` instead of numeric `id`.
Public endpoint with visibility check.

**Access rules:**
- `public` items: no authentication required.
- `private` items: requires authentication and ownership (or `superuser`).

Errors:
- `401` — private audio and no authentication provided.
- `403` — private audio and authenticated user is not the owner (and not `superuser`).
- `404` — audio not found, has no thumbnail, or thumbnail file missing on disk.

```bash
curl -o cover.webp http://localhost:3000/api/audio/thumb/AbC12XyZ
```

## DELETE /audio/{id}

Deletes an audio item (database record and file on disk). Owner or `superuser` only.
Requires authentication.

Response `200`:
```json
{
  "success": true,
  "data": "Audio deleted"
}
```

Errors:
- `403` — authenticated user does not own the audio.
- `404` — audio not found.
- `500` — database or file system error.

```bash
curl -X DELETE http://localhost:3000/api/audio/1 \
  -H "Authorization: Bearer <token>"
```
