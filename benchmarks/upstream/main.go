// Mock upstream server for gateway benchmarks.
// Returns configurable responses with minimal latency.
package main

import (
	"flag"
	"fmt"
	"log"
	"net/http"
	"strings"
)

var (
	port     = flag.Int("port", 9999, "Listen port")
	bodySize = flag.Int("body-size", 256, "Response body size in bytes")
)

func main() {
	flag.Parse()

	body := strings.Repeat("x", *bodySize)
	jsonBody := fmt.Sprintf(`{"status":"ok","data":"%s"}`, strings.Repeat("a", *bodySize-30))

	http.HandleFunc("/health", func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(200)
		w.Write([]byte(`{"status":"healthy"}`))
	})

	http.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(200)
		w.Write([]byte(jsonBody))
	})

	http.HandleFunc("/echo", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", r.Header.Get("Content-Type"))
		w.WriteHeader(200)
		buf := make([]byte, 1024*64)
		for {
			n, err := r.Body.Read(buf)
			if n > 0 {
				w.Write(buf[:n])
			}
			if err != nil {
				break
			}
		}
	})

	http.HandleFunc("/large", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(200)
		// ~10KB response for compression benchmarks
		w.Write([]byte(`{"data":"`))
		w.Write([]byte(strings.Repeat("benchmark-data-payload-", 400)))
		w.Write([]byte(`"}`))
	})

	_ = body
	addr := fmt.Sprintf(":%d", *port)
	log.Printf("Mock upstream listening on %s", addr)
	log.Fatal(http.ListenAndServe(addr, nil))
}
