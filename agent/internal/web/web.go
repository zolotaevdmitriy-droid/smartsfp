// Package web exposes the embedded static UI assets as an fs.FS.
// Lives next to the HTML/JS/CSS so the `//go:embed` directive (which
// can't reach into ../) works.
package web

import (
	"embed"
	"io/fs"
)

//go:embed index.html
var raw embed.FS

// FS returns the embedded UI assets rooted at the package directory,
// so index.html is at the FS root.
func FS() fs.FS {
	return raw
}
