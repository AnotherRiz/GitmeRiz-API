use axum::{
    body::Body,
    http::{header, HeaderMap, Response, StatusCode},
    response::IntoResponse,
    Json,
};
use crate::models::ApiResponse;

/// Check if the request accepts HTML based on the Accept header
pub fn accepts_html(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .map(|accept| accept.contains("text/html"))
        .unwrap_or(false)
}

/// Generate an HTML error page
fn generate_html_error_page(status_code: StatusCode, message: &str, frontend_url: &str) -> String {
    let status_num = status_code.as_u16();
    let title = match status_code {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            format!("{} | Unauthorized access to this image.", status_num)
        }
        StatusCode::NOT_FOUND => {
            format!("{} | Image not found.", status_num)
        }
        _ => {
            format!("{} | Error", status_num)
        }
    };

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{}</title>
    <style>
        * {{
            margin: 0;
            padding: 0;
            box-sizing: border-box;
        }}
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, Cantarell, sans-serif;
            background: linear-gradient(135deg, #1e3c72 0%, #2a5298 100%);
            color: #ffffff;
            display: flex;
            align-items: center;
            justify-content: center;
            min-height: 100vh;
            padding: 20px;
        }}
        .container {{
            text-align: center;
            max-width: 600px;
            background: rgba(255, 255, 255, 0.1);
            backdrop-filter: blur(10px);
            border-radius: 20px;
            padding: 60px 40px;
            box-shadow: 0 8px 32px rgba(0, 0, 0, 0.3);
        }}
        h1 {{
            font-size: 3rem;
            font-weight: 700;
            margin-bottom: 20px;
            line-height: 1.2;
        }}
        .status-code {{
            font-size: 6rem;
            font-weight: 900;
            color: rgba(255, 255, 255, 0.9);
            margin-bottom: 10px;
        }}
        p {{
            font-size: 1.2rem;
            margin-bottom: 40px;
            color: rgba(255, 255, 255, 0.9);
            line-height: 1.6;
        }}
        .btn {{
            display: inline-block;
            padding: 14px 40px;
            background: #ffffff;
            color: #1e3c72;
            text-decoration: none;
            border-radius: 50px;
            font-weight: 600;
            font-size: 1rem;
            transition: all 0.3s ease;
            box-shadow: 0 4px 15px rgba(0, 0, 0, 0.2);
        }}
        .btn:hover {{
            background: #f0f0f0;
            transform: translateY(-2px);
            box-shadow: 0 6px 20px rgba(0, 0, 0, 0.3);
        }}
        .error-details {{
            margin-top: 30px;
            font-size: 0.9rem;
            color: rgba(255, 255, 255, 0.7);
        }}
    </style>
</head>
<body>
    <div class="container">
        <div class="status-code">{}</div>
        <h1>{}</h1>
        <p>{}</p>
        <a href="{}" class="btn">Back to Home</a>
    </div>
</body>
</html>"#,
        title,
        status_num,
        title,
        message,
        frontend_url
    )
}

/// Build an error response with content negotiation
/// Returns HTML for browsers (Accept: text/html) or JSON for API clients
pub fn build_error_response(
    status_code: StatusCode,
    message: &str,
    headers: &HeaderMap,
    frontend_url: &str,
) -> Response<Body> {
    if accepts_html(headers) {
        // Return HTML error page for browsers
        let html = generate_html_error_page(status_code, message, frontend_url);
        Response::builder()
            .status(status_code)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .body(Body::from(html))
            .unwrap_or_else(|_| {
                // Fallback if HTML response building fails
                Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from("Internal server error"))
                    .unwrap()
            })
    } else {
        // Return JSON error for API clients
        (status_code, Json(ApiResponse::<()>::error(message))).into_response()
    }
}
