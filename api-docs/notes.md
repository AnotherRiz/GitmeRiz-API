# Notes

[← Back to Index](README.md)

All endpoints require authentication. Every note is scoped to its owner — you can
only see and modify your own notes, regardless of role.

## GET /notes

Lists the current user's notes.

Response `200`:
```json
{
  "success": true,
  "data": [
    { "id": 1, "user_id": 1, "title": "Todo", "content": "Buy milk" }
  ]
}
```

## POST /notes

Creates a note for the current user.

Request body:
```json
{
  "title": "Todo",
  "content": "Buy milk"
}
```

Response `201`:
```json
{
  "success": true,
  "data": { "id": 1, "user_id": 1, "title": "Todo", "content": "Buy milk" }
}
```

## GET /notes/{id}

Returns a single note owned by the current user, otherwise `404`.

## PUT /notes/{id}

Updates a note owned by the current user.

Request body:
```json
{
  "title": "Todo (edited)",
  "content": "Buy milk and eggs"
}
```

Response `200`: the updated note object.

## DELETE /notes/{id}

Deletes a note owned by the current user.

Response `200`:
```json
{ "success": true, "data": "Note deleted" }
```
