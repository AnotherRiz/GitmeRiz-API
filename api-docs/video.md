# Video

[← Back to Index](README.md)

Video endpoints let users upload, list, stream, and manage video files. All roles can
upload their own videos. A `user`/`admin` can only modify their own items; a
`superuser` can modify any item. Files are uploaded with `multipart/form-data` using
stream-to-disk to prevent memory overload, saved to disk under `STORAGE_DIR`, and
tracked in the database.

Each video item stores a `visibility` (`public` or `private`), a unique
8-character `short_id` for shareable URLs, an optional `thumbnail_path`, a
`transcoded_path` (for non-web-safe originals), a `pinned` flag, a `pin_order`,
and a `status`.

## Upload Constraints (Video)

- **Size limit:** No strict limit (backend streams chunks directly to disk to prevent OOM).
- **Allowed extensions:** `.mp4`, `.webm`, `.mov`, `.avi`, `.mkv`
- Bulk upload limit: Up to **5 files** in a single request.

## Background Processing & Transcoding

- Video processing is CPU-bound and runs **asynchronously in the background** after the upload response is returned.
- Concurrent FFmpeg tasks are limited to **1–2** via `tokio::sync::Semaphore`.
- The upload endpoint responds with `202 Accepted` as soon as the raw files are saved.
- **FFmpeg Pipeline:**
  1. **Thumbnail Extraction:** Extracts a single frame (tries `00:00:01`, falls back to `00:00:00` for short videos) and saves it as a WebP image (max width 1280px).
  2. **Transcoding (If needed):** If the uploaded video is not web-safe (e.g., `.mkv` or `.avi`), FFmpeg transcodes it to `.mp4` (H.264 video, AAC audio, `-movflags +faststart`).
- Freshly uploaded items start with `status: "processing"`. Once FFmpeg finishes, `status` becomes `active`.
- Poll `POST /video/status` to detect when processing has finished.

## Video Streaming (Range Requests)

- Served via `GET /video/r/{short_id}`.
- The backend fully supports **HTTP 206 Partial Content** and `Range` headers. This allows the frontend video player to scrub/seek to the middle of a large video instantly.

## Video Item Shape

```json
{
  "id": 1,
  "user_id": 1,
  "title": "Vacation Clip",
  "description": "A beautiful view of the beach",
  "original_filename": "raw_footage.mkv",
  "stored_path": "video/2026/06/2026-06-30/2026-06-30_14-25-10_UUID.mkv",
  "size_bytes": 104857600,
  "mime_type": "video/x-matroska",
  "visibility": "public",
  "short_id": "vX9mP2qL",
  "thumbnail_path": "video/2026/06/.../UUID-thumb.webp",
  "transcoded_path": "video/2026/06/.../UUID-web.mp4",
  "pinned": false,
  "status": "active",
  "pin_order": 0
}
```

*Note: `transcoded_path` is present only if the original was non-web-safe. The streaming/download endpoints automatically serve the transcoded file when available.*

---

## GET /video/public

Lists all **public** videos with **cursor-based pagination**, newest first. Public endpoint.

**Query Parameters:**

* `cursor` (optional): The `id` of the last item from the previous page.
* `limit` (optional): Number of items per page. Defaults to `20`. Maximum `50`.

Response `200`:

```json
{
  "success": true,
  "data": {
    "items": [ /* VideoItem[] */ ],
    "next_cursor": 450,
    "limit": 20
  }
}
```

## GET /video/me

Lists the current user's videos (public and private) with **cursor-based pagination**, newest first. **Requires authentication.**

Response `200`: Paginated envelope of the authenticated user's videos.

## GET /video/me/pinned

Lists the current user's **pinned** videos, ordered by `pin_order ASC, updated_at DESC`. **Requires authentication.** No pagination (max 4 items).

Response `200`: Array of video items where `pinned` is `true`.

## POST /video/status

Checks the processing `status` of multiple videos in a single request (polling). **Requires authentication.**

Request body:

```json
{ "ids": [1, 2] }
```

Response `200`:

```json
{
  "success": true,
  "data": {
    "1": "active",
    "2": "processing"
  }
}
```

## POST /video

Uploads video file(s). **Requires authentication.** Uses `multipart/form-data`:

| Field | Required | Description |
| --- | --- | --- |
| `file` | Yes | The video file(s). Bulk upload max 5 files. |
| `title` | No | Display title (used for single uploads). |
| `description` | No | Video description. |
| `visibility` | No | `public` or `private`. Defaults to `private`. |

**Upload process:**

1. Streams file directly to disk (prevents RAM spikes).
2. Generates a unique `short_id` and inserts metadata into the database (`status: "processing"`).
3. **Returns `202 Accepted` immediately.**
4. In the background: FFmpeg extracts a WebP thumbnail and transcodes the video to `.mp4` if necessary. Updates status to `active` or `failed_processing`.

Response `202` (single file):

```json
{
  "success": true,
  "data": {
    "id": 1,
    "title": "Vacation Clip",
    "short_id": "vX9mP2qL",
    "status": "processing",
    ...
  }
}
```

## GET /video/{id}

Returns a single video's metadata by numeric id. Public endpoint for public videos.
* **Access rules**: Public videos are accessible to everyone. Private videos require Cookie (`auth_token`) or `Authorization: Bearer` header, and will return `401 Unauthorized` or `403 Forbidden` if unauthorized.

## GET /video/d/{id}

Downloads the video file with `Content-Disposition: attachment` using the numeric database ID. Public endpoint for public videos. Serves transcoded file if available.
* **Access rules**: Same as `GET /video/{id}`.

## GET /video/info/{short_id}

Returns a single video's metadata using the 8-character `short_id`. Public endpoint for public videos. (Convenient for watch/detail pages when numeric ID is not known).
* **Access rules**: Same as `GET /video/{id}`.

## GET /video/download/{short_id}

Downloads the video file with `Content-Disposition: attachment` using the 8-character `short_id`. Public endpoint for public videos. Serves transcoded file if available.
* **Access rules**: Same as `GET /video/{id}`.

## GET /video/r/{short_id}

Serves the video file inline for HTML5 `<video>` tags. Supports HTTP `Range` requests for scrubbing.

* **Private videos**: Requires Cookie (`auth_token`) or `Authorization: Bearer` header.

## GET /video/t/{short_id}

Serves the **pre-generated thumbnail** (WebP) extracted from the video by FFmpeg. Cached for 1 year. Access rules match `GET /video/r/{short_id}`.

## PATCH /video/{id}

Unified partial update endpoint. Updates any combination of **title**, **description**, **visibility**, and **pinned** status. **Requires authentication.** Owner or superuser only.

Request body (all optional):

```json
{
  "title": "New Title",
  "description": "Updated description text",
  "visibility": "public",
  "pinned": true
}
```

## PATCH /video/reorder-pins

Persists drag-and-drop order for pinned videos (max 4 IDs). **Requires authentication.** Owner or superuser only.

Request body:

```json
{ "ordered_ids": [3, 1, 4, 2] }
```

## DELETE /video/{id}

Deletes a video (database record, original file, transcoded file, and thumbnail). **Requires authentication.** Owner or superuser only.

## POST /video/{id}/reprocess

Retries FFmpeg thumbnail extraction and transcoding. Returns `202 Accepted` immediately. **Requires authentication.** Owner or superuser only.
