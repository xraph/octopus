// Request Logger Script
// Logs all incoming requests with detailed information

let request_id = uuid();
let start_time = unix_time();

// Add request ID to headers
headers["X-Request-ID"] = request_id;

// Build log message
let log_msg = `[${request_id}] ${method} ${uri}`;

// Add client info if available
if headers.contains("x-forwarded-for") {
    log_msg += ` from ${headers["x-forwarded-for"]}`;
}

// Add user agent
if headers.contains("user-agent") {
    log_msg += ` (${headers["user-agent"]})`;
}

// Log the request
log_info(log_msg);

// Add timing header
headers["X-Request-Start-Time"] = start_time.to_string();

// Continue processing
true



