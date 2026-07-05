# Users

[← Back to Index](README.md)

All user endpoints require authentication.

---

## Validation Rules

When registering or updating user information, the following validation rules apply:

| Field | Rules |
|-------|-------|
| `username` | Min 3, Max 20 characters. Only letters, numbers, and underscore (`_`). No spaces. |
| `name` | Min 2, Max 50 characters. Spaces allowed. |
| `email` | Must be a valid email format. Max 255 characters. |
| `password` | Min 8 characters. Recommended: at least 1 uppercase letter and 1 digit. |

---

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

---

## GET /users

Lists all users with pagination. **Superuser only** (others get `403`).

**Query Parameters:**
- `page` (optional): Page number, defaults to `1`. Minimum `1`.
- `limit` (optional): Items per page, defaults to `20`. Maximum `100`.

**Example:** `GET /users?page=2&limit=10`

Response `200`:
```json
{
  "success": true,
  "data": {
    "users": [
      { "id": 1, "name": "John Doe", "username": "john", "email": "john@example.com", "role": "user" },
      { "id": 2, "name": "Admin", "username": "admin", "email": "admin@example.com", "role": "admin" }
    ],
    "page": 1,
    "limit": 20,
    "total": 1043
  }
}
```

**Notes:**
- The `limit` is clamped to a maximum of 100 to prevent server overload.
- The response includes pagination metadata: `page`, `limit`, and `total` count.

---

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

---

## PATCH /users/{id}

Partially updates a user's profile (name, username, email only). A user can only update their own profile; a superuser can update anyone. Otherwise `403`.

**All fields are optional** — only send the fields you want to change.

**Security Notes:**
- Password cannot be changed via this endpoint. Use `PATCH /users/me/password` instead.
- The `role` field is ignored even if sent. Roles cannot be changed via profile update.

Request body (all fields optional):
```json
{
  "name": "John Updated",
  "username": "john_new",
  "email": "john.new@example.com"
}
```

Response `200`: the updated user object (same shape as `GET /users/{id}`).

Response `400` if validation fails:
```json
{
  "success": false,
  "error": "Username must be 3-20 characters (letters, numbers, underscore only)"
}
```

---

## PATCH /users/me/password

Changes the authenticated user's password. **Requires current password for verification.**

Request body:
```json
{
  "current_password": "oldsecret123",
  "new_password": "NewSecret456"
}
```

Response `200`:
```json
{
  "success": true,
  "data": "Password updated successfully"
}
```

Response `401` if current password is incorrect:
```json
{
  "success": false,
  "error": "Current password is incorrect"
}
```

Response `400` if new password doesn't meet requirements:
```json
{
  "success": false,
  "error": "Password must be at least 8 characters"
}
```

**Example:**
```bash
curl -X PATCH http://localhost:3000/users/me/password \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"current_password":"old123","new_password":"NewPass123"}'
```

---

## DELETE /users/{id}

Deletes a user. **Superuser only** (others get `403`).

Response `200`:
```json
{
  "success": true,
  "data": "User deleted"
}
```
