# Video

[← Back to Index](README.md)

All video endpoints require authentication. Uploaded via `multipart/form-data`.
A `user`/`admin` can only view and delete their own videos; a `superuser` can view
all users' videos and delete any. Unlike Gallery, there are no public video
endpoints and videos have no `visibility` or `short_id` fields.

**Upload constraints (video):**
- No size limit.
- Allowed extensions: `.mov`, `.avi`, `.mkv`, `.mp4`, `.webm`

## GET /video

Lists videos. `superuser` sees all, others see only their own.

Response `200`:
```json
{
  "success": true,
  "data": [
    {
      "id": 1,
      "user_id": 1,
      "title": "My Clip",
      "original_filename": "clip.mp4",
      "stored_path": "video/2026/06/2026-06-30/2026-06-30_14-25-10_7c9e6679-7425-40de-944b-e07fc1f90ae7.mp4",
      "size_bytes": 10485760,
      "mime_type": "video/mp4"
    }
  ]
}
```

## POST /video

Uploads a video file for the current user. Allowed for all roles.
`multipart/form-data` fields: `file` (required), `title` (optional).

Response `201`:
```json
{
  "success": true,
  "data": {
    "id": 1,
    "user_id": 1,
    "title": "My Clip",
    "original_filename": "clip.mp4",
    "stored_path": "video/2026/06/2026-06-30/2026-06-30_14-25-10_7c9e6679-7425-40de-944b-e07fc1f90ae7.mp4",
    "size_bytes": 10485760,
    "mime_type": "video/mp4"
  }
}
```

Errors:
- `400` — no file provided, missing filename, or unsupported extension.

```bash
curl -X POST http://localhost:3000/video \
  -H "Authorization: Bearer <token>" \
  -F "title=My Clip" \
  -F "file=@clip.mp4"
```

## GET /video/{id}

Returns a single video's metadata. Owner or superuser only.

## GET /video/{id}/download

Downloads the actual video file. Owner or superuser only.

## DELETE /video/{id}

Deletes a video (database record and file on disk). Owner or superuser only.
