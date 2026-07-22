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
      "thumbnail_path": "audio/2026/07/2026-07-22/2026-07-22_14-26-40_3fa85f64-5717-4562-b3fc-2c963f66afa6-thumb.webp"
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
      "thumbnail_path": null
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
    "thumbnail_path": "audio/2026/07/2026-07-22/2026-07-22_14-26-40_a1b2c3d4-e5f6-7890-abcd-ef1234567890-thumb.webp"
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
    "thumbnail_path": null
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
  http://localhost:3000/api/audio/2/download
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
