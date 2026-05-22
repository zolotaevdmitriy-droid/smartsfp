// Package web exposes the embedded SPA assets as an fs.FS.
//
// The Svelte 5 SPA lives in /web-ui/. The build pipeline (in dev.sh) runs
// `vite build` and then `cp -r web-ui/dist agent/internal/web/dist` so
// that go:embed below picks up a static snapshot of the production bundle.
//
// We use hash-based routing inside the SPA, so http.FileServer can serve
// the assets directly without any history-fallback gymnastics: the
// browser only ever requests / and /assets/*.

package web

import (
	"embed"
	"io/fs"
)

//go:embed all:dist
var raw embed.FS

// FS returns the embedded UI assets rooted at the dist directory,
// so index.html is at the FS root.
func FS() fs.FS {
	sub, err := fs.Sub(raw, "dist")
	if err != nil {
		// embed always succeeds at compile time; the only way this can
		// fail at runtime is if the build pipeline didn't produce dist/.
		panic("acm-agent: embedded UI is missing — did you `npm run build` web-ui?")
	}
	return sub
}
