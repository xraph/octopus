// Response Transformation Script
// Wraps API responses in a standard envelope format

// Only transform successful JSON responses
if status >= 200 && status < 300 {
    
    // Check if response is JSON
    let content_type = headers.get("content-type", "");
    if content_type.contains("application/json") {
        
        // Parse response body
        let response_data = parse_json(body);
        
        // Create standard envelope
        let envelope = #{
            success: true,
            status_code: status,
            data: response_data,
            meta: #{
                timestamp: unix_time(),
                request_id: headers.get("x-request-id", "unknown"),
                version: "1.0",
                processed_by: "octopus-gateway"
            }
        };
        
        // Serialize back to JSON
        body = to_json(envelope);
        
        // Update content-length
        headers["content-length"] = body.len().to_string();
        
        log_debug("Response wrapped in envelope");
    }
    
} else if status >= 400 {
    // Transform error responses
    let error_envelope = #{
        success: false,
        status_code: status,
        error: #{
            message: "Request failed",
            timestamp: unix_time(),
            request_id: headers.get("x-request-id", "unknown")
        }
    };
    
    body = to_json(error_envelope);
    headers["content-type"] = "application/json";
    headers["content-length"] = body.len().to_string();
    
    log_debug(`Error response transformed: ${status}`);
}

true



