# Authentication

[← Back to Index](README.md)

See [Conventions → Authentication](conventions.md#authentication) for the overall
auth flow and how tokens/cookies are used.

## POST /register

Public. Registers a new user with the default role `user`.

**Validation Rules:**
- **username**: 3-20 characters, only letters, numbers, and underscore (`_`). No spaces.
- **name**: 2-50 characters, spaces allowed.
- **email**: Must be a valid email format, max 255 characters.
- **password**: Minimum 8 characters. Recommended: at least 1 uppercase letter and 1 digit.

Request body:
```json
{
  "name": "John Doe",
  "username": "john",
  "email": "john@example.com",
  "password": "secret123"
}
```

Response `201`:
```json
{
  "success": true,
  "data": {
    "id": 1,
    "name": "John Doe",
    "username": "john",
    "email": "john@example.com",
    "role": "user"
  }
}
```

Errors:
- `400` — missing fields, validation failed, or username/email already exists.

**Example validation error response:**
```json
{
  "success": false,
  "error": "Username must be 3-20 characters (letters, numbers, underscore only)"
}
```

```bash
curl -X POST http://localhost:3000/register \
  -H "Content-Type: application/json" \
  -d '{"name":"John Doe","username":"john","email":"john@example.com","password":"secret123"}'
```

## POST /login

Public. Verifies credentials, sets `auth_token` (Access Token, short-lived) and `refresh_token` (Refresh Token, long-lived) HttpOnly cookies, and returns user info + both tokens in JSON response.

Request body:
```json
{
  "username": "john",
  "password": "secret123"
}
```

Response `200`:
```json
{
  "success": true,
  "data": {
    "message": "Login successful",
    "token": "eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9...",
    "refresh_token": "f47ac10b-58cc-4372-a567-0e02b2c3d479",
    "user": {
      "id": 1,
      "name": "John Doe",
      "username": "john",
      "email": "john@example.com",
      "role": "user"
    }
  }
}
```

On success the server also sends cookies:
```
Set-Cookie: auth_token=eyJ0eXAi...; HttpOnly; SameSite=Lax; Path=/; Max-Age=900 (15 minutes)
Set-Cookie: refresh_token=f47ac10b...; HttpOnly; SameSite=Lax; Path=/; Max-Age=31536000 (1 year)
```

Errors:
- `401` — invalid credentials.

```bash
curl -X POST http://localhost:3000/login \
  -H "Content-Type: application/json" \
  -d '{"username":"john","password":"secret123"}'
```

## POST /logout

Public. Revokes the user session in the database and clears both `auth_token` and `refresh_token` cookies. Safe to call even without a valid session.

Response `200`:
```json
{
  "success": true,
  "data": "Logged out successfully"
}
```

The response expires the cookies:
```
Set-Cookie: auth_token=; HttpOnly; Path=/; Max-Age=0
Set-Cookie: refresh_token=; HttpOnly; Path=/; Max-Age=0
```

```bash
curl -X POST http://localhost:3000/logout
```

## POST /refresh

Public. Rotates the Access Token (JWT) and Refresh Token by validating the current `refresh_token` cookie.

Request:
Requires the `refresh_token` HttpOnly cookie to be present.

Response `200`:
```json
{
  "success": true,
  "data": {
    "message": "Token refreshed successfully",
    "token": "eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9...",
    "refresh_token": "a57de98b-18cc-4372-b567-1e02c2d3e589",
    "user": {
      "id": 1,
      "name": "John Doe",
      "username": "john",
      "email": "john@example.com",
      "role": "user"
    }
  }
}
```

On success the server also sends cookies:
```
Set-Cookie: auth_token=eyJ0eXAi...; HttpOnly; SameSite=Lax; Path=/; Max-Age=900 (15 minutes)
Set-Cookie: refresh_token=a57de98b...; HttpOnly; SameSite=Lax; Path=/; Max-Age=31536000 (1 year)
```

Errors:
- `400` — missing `refresh_token` cookie.
- `401` — invalid, expired (beyond 1 year), or revoked session.

```bash
curl -X POST http://localhost:3000/refresh
```

## GET /validate-session

Public. Checks whether the `refresh_token` HttpOnly cookie corresponds to a valid
(non-revoked, non-expired) session and returns the user info. Unlike `POST /refresh`,
this endpoint is read-only: it does **not** issue a new access token or rotate the
refresh token. Intended for the frontend to restore a session on app startup.

Response `200`:
```json
{
  "success": true,
  "data": {
    "id": 1,
    "name": "John Doe",
    "username": "john",
    "email": "john@example.com",
    "role": "user"
  }
}
```

Errors:
- `401` — missing `refresh_token` cookie, or the session is invalid/revoked/expired.

```bash
curl -b cookies.txt http://localhost:3000/validate-session
```
