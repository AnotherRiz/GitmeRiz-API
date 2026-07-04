# Authentication

[← Back to Index](README.md)

See [Conventions → Authentication](conventions.md#authentication) for the overall
auth flow and how tokens/cookies are used.

## POST /register

Public. Registers a new user with the default role `user`.

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
- `400` — missing fields, or username/email already exists.

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
