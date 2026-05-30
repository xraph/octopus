// Minimal Forge app wrapping Bastion gateway for benchmarking.
// Starts bastion on configurable port proxying to upstream on :9999.
package main

import (
	"fmt"
	"log"
	"os"

	"github.com/xraph/bastion"
	bastionExt "github.com/xraph/bastion/extension"
	"github.com/xraph/forge"
)

func main() {
	port := "8081"
	upstreamPort := "9999"
	if p := os.Getenv("BASTION_PORT"); p != "" {
		port = p
	}
	if p := os.Getenv("UPSTREAM_PORT"); p != "" {
		upstreamPort = p
	}

	upstreamURL := fmt.Sprintf("http://localhost:%s", upstreamPort)

	app := forge.New(
		forge.WithAppName("bastion-bench"),
		forge.WithHTTPAddress(fmt.Sprintf(":%s", port)),
		forge.WithExtensions(
			bastionExt.New(
				bastion.WithConfig(bastion.Config{
					Enabled:  true,
					BasePath: "",
					Routes: []bastion.RouteConfig{
						{
							Path:    "/*",
							Methods: []string{"GET", "POST", "PUT", "DELETE"},
							Targets: []bastion.TargetConfig{
								{URL: upstreamURL, Weight: 1},
							},
							Enabled: true,
						},
					},
				}),
			),
		),
	)

	log.Printf("Starting Bastion benchmark harness on :%s (upstream: %s)", port, upstreamURL)
	if err := app.Run(); err != nil {
		log.Fatal(err)
	}
}
