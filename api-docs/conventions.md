# Conventions

[← Back to Index](README.md)

## File Storage

Uploaded media files (Gallery, Video, Audio) are saved to disk under the
directory configured by `STORAGE_DIR` in `.env`. Files are organized as:

```
{type}/{YYYY}/{MM}/{YYYY-MM-DD}/YYYY-MM-DD_HH-MM-SS_UUID.<ext>
```

For example:
```
gallery/2026/06/2026-06-30/2026-06-30_14-23-05_550e8400-e29b-41d4-a716-446655440000.jpg
```

The database stores the relative `stored_path`, the `original_filename`,
`size_bytes`, and `mime_type` for each item. The server always generates the
on-disk filename; the client-provided name is only kept as `original_filename`.
Gallery items additionally store a `visibility` (`public` or `private`) and a
unique 8-character `short_id` for shareable URLs.

## Response Envelope

All endpoints return a consistent JSON structure.

Success:
```json
{
  "success": true,
  "data": { ... }
}
```

Error:
```json
{
  "success": false,
  "error": "Error message"
}
```

## Status Codes

| Code | Meaning |
|------|---------|
| `200 OK` | Request succeeded |
| `201 Created` | Resource created |
| `202 Accepted` | Upload accepted; media saved and queued for background processing (see [Gallery](gallery.md)) |
| `400 Bad Request` | Invalid input / duplicate entry / invalid file extension |
| `401 Unauthorized` | Missing or invalid token |
| `403 Forbidden` | Authenticated but insufficient role |
| `404 Not Found` | Resource not found |
| `413 Payload Too Large` | Uploaded image exceeds the 100 MB limit |
| `500 Internal Server Error` | Server/database error |

## Authentication

1. Call `POST /login` to authenticate. On success the server:
   - Sets an `auth_token` HttpOnly cookie (`SameSite=Lax`, `Path=/`, 365-day expiry).
   - Also returns the raw token in the response body for non-browser clients.
2. Subsequent protected requests are authenticated in this order:
   - **Cookie (preferred)**: the `auth_token` cookie is sent automatically by the browser.
   - **Header (fallback)**: `Authorization: Bearer <token>` for API clients that can't use cookies.
   ```
   Authorization: Bearer eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9...
   ```
3. Tokens are valid for 365 days (1 year). Call `POST /logout` to clear the cookie.

**CORS note:** Credentialed requests are only accepted from `http://localhost:5173`.
Browser clients must send requests with credentials enabled (e.g. `fetch(..., { credentials: 'include' })`).

## Roles & Permissions

| Resource | `user` | `admin` | `superuser` |
|----------|--------|---------|-------------|
| Gallery — list public images (`GET /gallery/public`) | Public | Public | Public |
| Gallery — list own images (`GET /gallery/me`) | Yes (own) | Yes (own) | Yes (own) |
| Gallery — upload own | Yes | Yes | Yes |
| Gallery — edit/delete (title, visibility, delete) | Own only | Own only | Any |
| Video / Audio — upload own | Yes | Yes | Yes |
| Video / Audio — view own items | Yes | Yes | Yes |
| Video / Audio — view all users' items | No | No | Yes |
| Video / Audio — delete | Own only | Own only | Any |
| Blog — read (requires auth) | Yes | Yes | Yes |
| Blog — write (create/update/delete) | No | Yes | Yes |
| Notes — read & write (own) | Yes | Yes | Yes |
| Clipboard — access (own) | Yes | Yes | Yes |
| List all users | No | No | Yes |
| Delete a user | No | No | Yes |

New accounts always start with the `user` role.
