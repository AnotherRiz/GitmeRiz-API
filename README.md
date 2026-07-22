# GitmeRiz API

A Rust backend API built with Axum, Tokio, and MySQL, featuring JWT authentication and role-based access control.

## Tech Stack

- **Web Framework**: Axum
- **Async Runtime**: Tokio
- **HTTP Middleware**: Tower-HTTP (CORS, logging)
- **Database**: MySQL (via SQLx)
- **Serialization**: Serde / Serde JSON
- **Security**: Bcrypt (password hashing), JWT (authentication)

## Project Structure

```
src/
├── main.rs          # Application entry point, router setup
├── config.rs        # Environment configuration
├── db.rs            # Database connection and migrations
├── models.rs        # Data structures, roles, and API response types
├── auth.rs          # Password hashing and JWT utilities
├── middleware.rs    # Authentication middleware
├── media.rs         # Media file utilities (paths, validation, image processing)
├── error_page.rs    # HTML/JSON error responses (content negotiation)
└── handlers/
    ├── mod.rs       # Handler module exports
    ├── health.rs    # Health check endpoint
    ├── users.rs     # User auth and CRUD endpoints
    ├── gallery.rs   # Image gallery endpoints
    ├── video.rs     # Video endpoints
    ├── audio.rs     # Audio endpoints
    ├── blog.rs      # Blog post endpoints
    ├── notes.rs     # Notes endpoints
    └── clipboard.rs # Clipboard endpoints

api-docs/            # Full API reference (see "API Documentation" below)
```

## Roles & Permissions

| Resource | `user` | `admin` | `superuser` |
|----------|--------|---------|-------------|
| Gallery — upload own image | ✓ | ✓ | ✓ |
| Gallery — view all users' images | ✗ | ✗ | ✓ |
| Video — upload own video | ✓ | ✓ | ✓ |
| Video — view all users' videos | ✗ | ✗ | ✓ |
| Audio — upload own audio | ✓ | ✓ | ✓ |
| Audio — view all users' audio | ✗ | ✗ | ✓ |
| Blog — read | ✓ | ✓ | ✓ |
| Blog — write | ✗ | ✓ | ✓ |
| Notes — read & write | ✓ | ✓ | ✓ |
| Clipboard — access | ✓ | ✓ | ✓ |

## Prerequisites

- Rust (1.70+)
- MySQL server running locally or remotely

## Setup

1. Clone the repository.

2. Copy `.env.example` to `.env` and update the values:
   ```
   DATABASE_URL=mysql://user:password@localhost:3306/gitmeriz
   SERVER_HOST=localhost
   SERVER_PORT=3000
   JWT_SECRET=your-super-secret-jwt-key-change-in-production
   STORAGE_DIR=./storage
   FRONTEND_URL=http://localhost:5173
   ```
   
   **Environment Variables:**
   - `DATABASE_URL` — MySQL connection string
   - `SERVER_HOST` — Server host (default: `localhost`)
   - `SERVER_PORT` — Server port (default: `3000`)
   - `JWT_SECRET` — Secret key for JWT token signing (change in production!)
   - `STORAGE_DIR` — Directory for uploaded media files (images, videos, audio)
   - `FRONTEND_URL` — Frontend application URL for error page navigation (default: `http://localhost:5173`)

3. Create the MySQL database:
   ```sql
   CREATE DATABASE gitmeriz;
   ```

4. Build and run:
   ```bash
   cargo build
   cargo run
   ```

The server will start at `http://localhost:3000`. All tables are created automatically on startup.

## API Documentation

Full API reference lives in the [`api-docs/`](api-docs/README.md) folder, split by resource for
easy reading:

| Document | Description |
|----------|-------------|
| [Overview & Index](api-docs/README.md) | Entry point with links to everything |
| [Conventions](api-docs/conventions.md) | Response envelope, status codes, auth flow, roles, file storage |
| [Authentication](api-docs/authentication.md) | Register, login, logout |
| [Users](api-docs/users.md) | User management |
| [Gallery](api-docs/gallery.md) | Image upload, background processing, thumbnails/previews, pinning, signed URLs |
| [Video](api-docs/video.md) | Video upload and management |
| [Audio](api-docs/audio.md) | Audio upload and management |
| [Blog](api-docs/blog.md) | Blog post CRUD |
| [Notes](api-docs/notes.md) | Personal notes CRUD |
| [Clipboard](api-docs/clipboard.md) | Clipboard items CRUD |
| [Health](api-docs/health.md) | Health check |
| [Endpoint Summary](api-docs/endpoint-summary.md) | Complete table of all endpoints |

## API Endpoints (Overview)

A high-level summary. See the [full documentation](api-docs/README.md) for request/response
details, error cases, and examples.

### Public (No Auth Required)
- `GET /health` — Health check
- `POST /register` — Register a new user (default role: `user`)
- `POST /login` — Login; sets an `auth_token` cookie and returns a JWT
- `POST /logout` — Clear the `auth_token` cookie
- `GET /gallery/public` — List all public images
- `GET /gallery/{id}` — Get image metadata
- `GET /gallery/d/{id}` — Download image file
- `GET /gallery/r|t|p/{short_id}` — Serve raw / thumbnail / preview image (public, or private via signed URL)
- `GET /audio/public` — List all public audio
- `GET /audio/{id}` — Get audio metadata (public audio, or private with auth)
- `GET /audio/{id}/download` — Download audio file (public audio, or private with auth)

### Protected (Auth Required)
Authenticated via the `auth_token` cookie (preferred) or an `Authorization: Bearer <token>` header.

- **Users** — `GET /users/me`, `GET /users` (superuser), `GET|PUT /users/{id}`, `DELETE /users/{id}` (superuser)
- **Gallery** — `POST /gallery` (upload, returns `202`, processes in background), `GET /gallery/me`,
  `GET /gallery/me/pinned`, `POST /gallery/status`, `PATCH /gallery/{id}/title|visibility|pinned`,
  `PATCH /gallery/reorder-pins`, `POST /gallery/{id}/reprocess`, `POST /gallery/{short_id}/sign`, `DELETE /gallery/{id}`
- **Video** — `GET /video`, `POST /video`, `GET /video/{id}`, `GET /video/{id}/download`, `DELETE /video/{id}`
- **Audio** — `GET /audio`, `POST /audio`, `DELETE /audio/{id}`
- **Blog** — `GET /blog`, `POST /blog` (admin/superuser), `GET /blog/{id}`, `PUT|DELETE /blog/{id}` (admin/superuser)
- **Notes** — `GET|POST /notes`, `GET|PUT|DELETE /notes/{id}`
- **Clipboard** — `GET|POST /clipboard`, `GET|PUT|DELETE /clipboard/{id}`

> Gallery uploads use **background processing**: the upload endpoint returns `202 Accepted`
> immediately, then generates thumbnails and previews asynchronously. See
> [Gallery docs](api-docs/gallery.md) for the full workflow.

## Request/Response Examples

### Register
```bash
curl -X POST http://localhost:3000/register \
  -H "Content-Type: application/json" \
  -d '{"name":"John","username":"john","email":"john@example.com","password":"secret123"}'
```

### Login
```bash
curl -X POST http://localhost:3000/login \
  -H "Content-Type: application/json" \
  -d '{"username":"john","password":"secret123"}'
```

Response:
```json
{
  "success": true,
  "data": {
    "message": "Login successful",
    "token": "eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9...",
    "user": {
      "id": 1,
      "name": "John",
      "username": "john",
      "email": "john@example.com",
      "role": "user"
    }
  }
}
```

### Using Protected Endpoints
```bash
curl http://localhost:3000/users/me \
  -H "Authorization: Bearer <your-token>"
```

### Response Format
All endpoints return a consistent JSON structure:
```json
{
  "success": true,
  "data": { ... }
}
```

Or on error:
```json
{
  "success": false,
  "error": "Error message"
}
```

## Error Codes
- `400 Bad Request` — Invalid input or validation error
- `401 Unauthorized` — Missing or invalid token
- `403 Forbidden` — Insufficient permissions for the action
- `404 Not Found` — Resource not found
- `413 Payload Too Large` — Uploaded image exceeds the 100 MB limit
- `500 Internal Server Error` — Server or database error

See [Conventions → Status Codes](api-docs/conventions.md#status-codes) for the full list,
including `202 Accepted` used by gallery uploads.

