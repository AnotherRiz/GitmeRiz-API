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

Public. Verifies credentials, sets an `auth_token` HttpOnly cookie, and returns a JWT token.

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

On success the server also sends a cookie:
```
Set-Cookie: auth_token=eyJ0eXAi...; HttpOnly; SameSite=Lax; Path=/; Max-Age=31536000
```

Errors:
- `401` — invalid credentials.

```bash
curl -X POST http://localhost:3000/login \
  -H "Content-Type: application/json" \
  -d '{"username":"john","password":"secret123"}'
```

## POST /logout

Public. Clears the `auth_token` cookie by expiring it. Safe to call even without a
valid session.

Response `200`:
```json
{
  "success": true,
  "data": "Logged out successfully"
}
```

The response expires the cookie:
```
Set-Cookie: auth_token=; HttpOnly; Path=/; Max-Age=0
```

```bash
curl -X POST http://localhost:3000/logout
```
