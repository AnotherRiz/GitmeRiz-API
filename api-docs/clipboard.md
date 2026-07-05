# Clipboard

[← Back to Index](README.md)

All endpoints require authentication. Every item is scoped to its owner.

## GET /clipboard

Lists the current user's clipboard items.

Response `200`:
```json
{
  "success": true,
  "data": [
    { "id": 1, "user_id": 1, "content": "Copied text" }
  ]
}
```

## POST /clipboard

Creates a clipboard item for the current user.

Request body:
```json
{
  "content": "Copied text"
}
```

Response `201`:
```json
{
  "success": true,
  "data": { "id": 1, "user_id": 1, "content": "Copied text" }
}
```

## GET /clipboard/{id}

Returns a single clipboard item owned by the current user, otherwise `404`.

## PUT /clipboard/{id}

Updates a clipboard item owned by the current user.

Request body:
```json
{
  "content": "Updated text"
}
```

Response `200`: the updated clipboard item.

## DELETE /clipboard/{id}

Deletes a clipboard item owned by the current user.

Response `200`:
```json
{ "success": true, "data": "Clipboard item deleted" }
```
