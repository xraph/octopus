// Request Validation Script
// Validates incoming requests for required headers and proper format

// Validate required headers for authenticated endpoints
if uri.starts_with("/api/") && !uri.starts_with("/api/public") {
    
    // Check for authentication
    if !headers.contains("authorization") {
        log_warn(`Missing Authorization header for: ${uri}`);
        return false;  // Reject request
    }
    
    let auth = headers["authorization"];
    if !auth.starts_with("Bearer ") {
        log_warn(`Invalid Authorization format for: ${uri}`);
        return false;
    }
    
    // Validate token length (basic check)
    let token = auth.sub_string(7);
    if token.len() < 20 {
        log_warn(`Invalid token length for: ${uri}`);
        return false;
    }
}

// Validate content-type for POST/PUT/PATCH
if method == "POST" || method == "PUT" || method == "PATCH" {
    if !headers.contains("content-type") {
        log_warn(`Missing Content-Type for ${method} ${uri}`);
        return false;
    }
    
    let content_type = headers["content-type"];
    if !content_type.contains("application/json") && 
       !content_type.contains("application/x-www-form-urlencoded") &&
       !content_type.contains("multipart/form-data") {
        log_warn(`Unsupported Content-Type: ${content_type}`);
        return false;
    }
}

// Validate request ID (add if missing)
if !headers.contains("x-request-id") {
    headers["x-request-id"] = uuid();
    log_debug("Generated request ID");
}

log_debug(`Validation passed for: ${method} ${uri}`);
true



