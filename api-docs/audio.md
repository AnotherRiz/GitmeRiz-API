# Audio

[← Back to Index](README.md)

All audio endpoints require authentication. Uploaded via `multipart/form-data`.
A `user`/`admin` can only view and delete their own audio; a `superuser` can view
all users' audio and delete any. Unlike Gallery, there are no public audio
endpoints and audio items have no `visibility` or `short_id` fields.

**Upload constraints (audio):**
- No size limit.
- Allowed extensions: `.flac`, `.wav`, `.m4a`, `.mp3`, `.aac`, `.ogg`

## GET /audio

Lists audio. `superuser` sees all, others see only their own.

Response `200`:
```json
{
  "success": true,
  "data": [
    {
      "id": 1,
      "user_id": 1,
      "title": "My Song",
      "original_filename": "song.mp3",
      "stored_path": "audio/2026/06/2026-06-30/2026-06-30_14-26-40_3fa85f64-5717-4562-b3fc-2c963f66afa6.mp3",
      "size_bytes": 5242880,
      "mime_type": "audio/mpeg"
    }
  ]
}
```

## POST /audio

Uploads an audio file for the current user. Allowed for all roles.
`multipart/form-data` fields: `file` (required), `title` (optional).

Response `201`:
```json
{
  "success": true,
  "data": {
    "id": 1,
    "user_id": 1,
    "title": "My Song",
    "original_filename": "song.mp3",
    "stored_path": "audio/2026/06/2026-06-30/2026-06-30_14-26-40_3fa85f64-5717-4562-b3fc-2c963f66afa6.mp3",
    "size_bytes": 5242880,
    "mime_type": "audio/mpeg"
  }
}
```

Errors:
- `400` — no file provided, missing filename, or unsupported extension.

```bash
curl -X POST http://localhost:3000/audio \
  -H "Authorization: Bearer <token>" \
  -F "title=My Song" \
  -F "file=@song.mp3"
```

## GET /audio/{id}

Returns a single audio item's metadata. Owner or superuser only.

## GET /audio/{id}/download

Downloads the actual audio file. Owner or superuser only.

## DELETE /audio/{id}

Deletes an audio item (database record and file on disk). Owner or superuser only.
