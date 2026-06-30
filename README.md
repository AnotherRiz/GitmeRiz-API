# GitmeRiz API

A Rust backend API built with Axum, Tokio, and MySQL.

## Tech Stack

- **Web Framework**: Axum
- **Async Runtime**: Tokio
- **HTTP Middleware**: Tower-HTTP (CORS, logging)
- **Database**: MySQL (via SQLx)
- **Serialization**: Serde / Serde JSON
- **Security**: Bcrypt (password hashing)

## Project Structure

```
src/
├── main.rs          # Application entry point
├── config.rs        # Environment configuration
├── db.rs            # Database connection and migrations
├── models.rs        # Data structures and API response types
├── auth.rs          # Password hashing utilities
└── handlers/
    ├── mod.rs       # Handler module exports
    ├── health.rs    # Health check endpoint
    └── users.rs     # User CRUD and auth endpoints
```

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

The server will start at `http://127.0.0.1:3000`. The `users` table is created automatically on startup.

## API Endpoints

### Health Check
- `GET /health` — Returns server status.

### Authentication
- `POST /api/register` — Register a new user.
- `POST /api/login` — Login with username and password.

### Users (CRUD)
- `GET /api/users` — List all users.
- `GET /api/users/:id` — Get a user by ID.
- `PUT /api/users/:id` — Update a user.
- `DELETE /api/users/:id` — Delete a user.

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
