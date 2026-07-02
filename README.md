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
   SERVER_HOST=localhost
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

The server will start at `http://localhost:3000`. All tables are created automatically on startup.

## API Endpoints

### Public (No Auth Required)
- `GET /health` — Health check
- `POST /register` — Register a new user (default role: `user`)
- `POST /login` — Login and receive JWT token

### Protected (Auth Required)
All protected endpoints require the `Authorization: Bearer <token>` header.

#### Users
- `GET /users/me` — Get current user
- `GET /users` — List all users (superuser only)
- `GET /users/:id` — Get user by ID
- `PUT /users/:id` — Update user
- `DELETE /users/:id` — Delete user (superuser only)

#### Gallery (Images)
- `GET /gallery` — List images (own images, or all for superuser)
- `POST /gallery` — Upload image
- `GET /gallery/:id` — Get image
- `DELETE /gallery/:id` — Delete image

#### Video
- `GET /video` — List videos (own videos, or all for superuser)
- `POST /video` — Upload video
- `GET /video/:id` — Get video
- `DELETE /video/:id` — Delete video

#### Audio
- `GET /audio` — List audio (own audio, or all for superuser)
- `POST /audio` — Upload audio
- `GET /audio/:id` — Get audio
- `DELETE /audio/:id` — Delete audio

#### Blog
- `GET /blog` — List published posts (all roles)
- `POST /blog` — Create post (admin/superuser only)
- `GET /blog/:id` — Get post
- `PUT /blog/:id` — Update post (admin/superuser only)
- `DELETE /blog/:id` — Delete post (admin/superuser only)

#### Notes
- `GET /notes` — List own notes
- `POST /notes` — Create note
- `GET /notes/:id` — Get note
- `PUT /notes/:id` — Update note
- `DELETE /notes/:id` — Delete note

#### Clipboard
- `GET /clipboard` — List own clipboard items
- `POST /clipboard` — Create clipboard item
- `GET /clipboard/:id` — Get clipboard item
- `PUT /clipboard/:id` — Update clipboard item
- `DELETE /clipboard/:id` — Delete clipboard item

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
- `401 Unauthorized` — Missing or invalid token
- `403 Forbidden` — Insufficient permissions for the action
- `404 Not Found` — Resource not found

