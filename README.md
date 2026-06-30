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
├── main.rs          # Application entry point
├── config.rs        # Environment configuration
├── db.rs            # Database connection and migrations
├── models.rs        # Data structures, roles, and API response types
├── auth.rs          # Password hashing and JWT utilities
├── middleware.rs    # Authentication middleware
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
   SERVER_HOST=127.0.0.1
   SERVER_PORT=3000
   JWT_SECRET=your-super-secret-jwt-key-change-in-production
   ```

3. Create the MySQL database:
   ```sql
   CREATE DATABASE gitmeriz;
   ```

4. Build and run:
   ```bash
   cargo build
   cargo run
   ```

The server will start at `http://127.0.0.1:3000`. All tables are created automatically on startup.

## API Endpoints

### Public (No Auth Required)
- `GET /health` — Health check
- `POST /api/register` — Register a new user (default role: `user`)
- `POST /api/login` — Login and receive JWT token

### Protected (Auth Required)
All protected endpoints require the `Authorization: Bearer <token>` header.

#### Users
- `GET /api/users/me` — Get current user
- `GET /api/users` — List all users (superuser only)
- `GET /api/users/:id` — Get user by ID
- `PUT /api/users/:id` — Update user
- `DELETE /api/users/:id` — Delete user (superuser only)

#### Gallery (Images)
- `GET /api/gallery` — List images (own images, or all for superuser)
- `POST /api/gallery` — Upload image
- `GET /api/gallery/:id` — Get image
- `DELETE /api/gallery/:id` — Delete image

#### Video
- `GET /api/video` — List videos (own videos, or all for superuser)
- `POST /api/video` — Upload video
- `GET /api/video/:id` — Get video
- `DELETE /api/video/:id` — Delete video

#### Audio
- `GET /api/audio` — List audio (own audio, or all for superuser)
- `POST /api/audio` — Upload audio
- `GET /api/audio/:id` — Get audio
- `DELETE /api/audio/:id` — Delete audio

#### Blog
- `GET /api/blog` — List published posts (all roles)
- `POST /api/blog` — Create post (admin/superuser only)
- `GET /api/blog/:id` — Get post
- `PUT /api/blog/:id` — Update post (admin/superuser only)
- `DELETE /api/blog/:id` — Delete post (admin/superuser only)

#### Notes
- `GET /api/notes` — List own notes
- `POST /api/notes` — Create note
- `GET /api/notes/:id` — Get note
- `PUT /api/notes/:id` — Update note
- `DELETE /api/notes/:id` — Delete note

#### Clipboard
- `GET /api/clipboard` — List own clipboard items
- `POST /api/clipboard` — Create clipboard item
- `GET /api/clipboard/:id` — Get clipboard item
- `PUT /api/clipboard/:id` — Update clipboard item
- `DELETE /api/clipboard/:id` — Delete clipboard item

## Request/Response Examples

### Register
```bash
curl -X POST http://127.0.0.1:3000/api/register \
  -H "Content-Type: application/json" \
  -d '{"name":"John","username":"john","email":"john@example.com","password":"secret123"}'
```

### Login
```bash
curl -X POST http://127.0.0.1:3000/api/login \
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
curl http://127.0.0.1:3000/api/users/me \
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
- `401 Unauthorized` — Missing or invalid token
- `403 Forbidden` — Insufficient permissions for the action
- `404 Not Found` — Resource not found
