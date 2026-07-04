# Users

[← Back to Index](README.md)

All user endpoints require authentication.

## GET /users/me

Returns the currently authenticated user.

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

```bash
curl http://localhost:3000/users/me \
  -H "Authorization: Bearer <token>"
```

## GET /users

Lists all users. **Superuser only** (others get `403`).

Response `200`:
```json
{
  "success": true,
  "data": [
    { "id": 1, "name": "John Doe", "username": "john", "email": "john@example.com", "role": "user" },
    { "id": 2, "name": "Admin", "username": "admin", "email": "admin@example.com", "role": "admin" }
  ]
}
```

## GET /users/{id}

Returns a single user. A user can only view their own profile; a superuser can
view anyone. Otherwise `403`.

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

## PUT /users/{id}

Updates a user. A user can only update their own profile; a superuser can update
anyone. Otherwise `403`.

Request body:
```json
{
  "name": "John Updated",
  "username": "john",
  "email": "john@example.com",
  "password": "newsecret123"
}
```

Response `200`: the updated user object (same shape as `GET /users/{id}`).

## DELETE /users/{id}

Deletes a user. **Superuser only** (others get `403`).

Response `200`:
```json
{
  "success": true,
  "data": "User deleted"
}
```
