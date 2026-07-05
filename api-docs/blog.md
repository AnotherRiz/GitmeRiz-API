# Blog

[← Back to Index](README.md)

All endpoints require authentication. Reading is allowed for everyone; writing
(create/update/delete) is restricted to `admin` and `superuser`.

## GET /blog

Lists published blog posts. Allowed for all roles.

Response `200`:
```json
{
  "success": true,
  "data": [
    { "id": 1, "author_id": 2, "title": "Hello", "content": "First post", "published": true }
  ]
}
```

## POST /blog

Creates a blog post. **Admin / superuser only** (`user` gets `403`).

Request body:
```json
{
  "title": "Hello",
  "content": "First post",
  "published": true
}
```
`published` is optional and defaults to `false`.

Response `201`:
```json
{
  "success": true,
  "data": { "id": 1, "author_id": 2, "title": "Hello", "content": "First post", "published": true }
}
```

## GET /blog/{id}

Returns a single post. Published posts are visible to everyone. Unpublished posts
are only visible to the author or users who can write blog posts; otherwise `404`.

## PUT /blog/{id}

Updates a post. **Admin / superuser only**.

Request body:
```json
{
  "title": "Hello (edited)",
  "content": "Updated content",
  "published": true
}
```

Response `200`: the updated post object.

## DELETE /blog/{id}

Deletes a post. **Admin / superuser only**.

Response `200`:
```json
{ "success": true, "data": "Blog post deleted" }
```
