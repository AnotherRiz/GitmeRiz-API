# Endpoint Summary

[← Back to Index](README.md)

| Method | Endpoint | Auth | Notes |
|--------|----------|------|-------|
| GET | `/health` | No | Health check |
| POST | `/register` | No | Register (role `user`), validates username/name/email/password |
| POST | `/login` | No | Login, sets `auth_token` & `refresh_token` cookies + returns tokens |
| POST | `/logout` | No | Clears `auth_token` & `refresh_token` cookies, revokes session in DB |
| POST | `/refresh` | No | Rotates Access Token and Refresh Token using `refresh_token` cookie |
| GET | `/users/me` | Yes | Current user |
| GET | `/users` | Yes | List users with pagination (superuser) - supports `?page=1&limit=20` |
| GET | `/users/{id}` | Yes | Get user (self or superuser) |
| PATCH | `/users/{id}` | Yes | Partial profile update: name/username/email (self or superuser) - validates input, no password/role |
| PATCH | `/users/me/password` | Yes | Change password - requires current password verification |
| DELETE | `/users/{id}` | Yes | Delete user (superuser) |
| GET | `/gallery/public` | No | List all public images with cursor pagination - supports `?cursor={id}&limit=50` |
| GET | `/gallery/me` | Yes | List current user's images (public & private) with cursor pagination - supports `?cursor={id}&limit=50` |
| GET | `/gallery/me/pinned` | Yes | List current user's pinned images (no pagination, max 8) |
| POST | `/gallery` | Yes | Upload image(s) (multipart, max 100 MB, up to 50 files); returns `202`, processes in background |
| POST | `/gallery/status` | Yes | Check processing status of up to 100 image ids (own only) |
| GET | `/gallery/{id}` | No | Get image metadata by numeric id (public) |
| GET | `/gallery/d/{id}` | No | Download image file with attachment header (force download) |
| GET | `/gallery/r/{short_id}` | Optional | Serve raw full-size image inline (public: no auth, private: signed URL/cookie/header) |
| GET | `/gallery/t/{short_id}` | Optional | Serve pre-generated thumbnail inline (WebP lossy, cached 1 year) |
| GET | `/gallery/p/{short_id}` | Optional | Serve pre-generated medium preview inline (WebP lossy, cached 1 hour) |
| PATCH | `/gallery/{id}` | Yes | Unified partial update: title/visibility/pinned (optional fields, owner/superuser) |
| PATCH | `/gallery/reorder-pins` | Yes | Persist custom order for pinned images (drag-and-drop support) (owner / superuser) |
| POST | `/gallery/{id}/reprocess` | Yes | Retry thumbnail + preview generation - returns `202`, processes in background, poll `/gallery/status` (owner / superuser) |
| DELETE | `/gallery/{id}` | Yes | Delete image + file (owner / superuser) |
| GET | `/video` | Yes | List videos (own / all for superuser) |
| POST | `/video` | Yes | Upload video (multipart, no size limit) |
| GET | `/video/{id}` | Yes | Get video metadata (owner / superuser) |
| GET | `/video/{id}/download` | Yes | Download video file (owner / superuser) |
| DELETE | `/video/{id}` | Yes | Delete video + file (owner / superuser) |
| GET | `/audio` | Yes | List audio (own / all for superuser) |
| POST | `/audio` | Yes | Upload audio (multipart, no size limit) |
| GET | `/audio/{id}` | Yes | Get audio metadata (owner / superuser) |
| GET | `/audio/{id}/download` | Yes | Download audio file (owner / superuser) |
| DELETE | `/audio/{id}` | Yes | Delete audio + file (owner / superuser) |
| GET | `/blog` | Yes | List published posts |
| POST | `/blog` | Yes | Create post (admin / superuser) |
| GET | `/blog/{id}` | Yes | Get post |
| PUT | `/blog/{id}` | Yes | Update post (admin / superuser) |
| DELETE | `/blog/{id}` | Yes | Delete post (admin / superuser) |
| GET | `/notes` | Yes | List own notes |
| POST | `/notes` | Yes | Create note |
| GET | `/notes/{id}` | Yes | Get own note |
| PUT | `/notes/{id}` | Yes | Update own note |
| DELETE | `/notes/{id}` | Yes | Delete own note |
| GET | `/clipboard` | Yes | List own clipboard items |
| POST | `/clipboard` | Yes | Create clipboard item |
| GET | `/clipboard/{id}` | Yes | Get own clipboard item |
| PUT | `/clipboard/{id}` | Yes | Update own clipboard item |
| DELETE | `/clipboard/{id}` | Yes | Delete own clipboard item |
