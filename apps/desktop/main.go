package main

import (
	"context"
	"embed"
	"fmt"
	"os"
	"os/signal"
	"syscall"

	"github.com/hkd987/OpenADE/apps/desktop/internal/daemon"
	"github.com/wailsapp/wails/v2"
	"github.com/wailsapp/wails/v2/pkg/options"
	"github.com/wailsapp/wails/v2/pkg/options/assetserver"
	"github.com/wailsapp/wails/v2/pkg/options/mac"
)

//go:embed all:dist
var assets embed.FS

func main() {
	if hasArgument("--daemon") {
		runDaemon()
		return
	}

	app := NewApp()
	err := wails.Run(&options.App{
		Title:            "OpenADE",
		Width:            1480,
		Height:           920,
		MinWidth:         1040,
		MinHeight:        680,
		DisableResize:    false,
		Fullscreen:       false,
		BackgroundColour: &options.RGBA{R: 14, G: 16, B: 19, A: 1},
		Mac:              &mac.Options{TitleBar: mac.TitleBarHiddenInset()},
		AssetServer:      &assetserver.Options{Assets: assets},
		OnStartup:        app.startup,
		OnShutdown:       app.shutdown,
		Bind:             []interface{}{app},
	})
	if err != nil {
		fmt.Fprintln(os.Stderr, "OpenADE:", err)
		os.Exit(1)
	}
}

func runDaemon() {
	config := daemon.DefaultConfig()
	if value := argumentValue("--data-dir"); value != "" {
		config.DataDir = value
	}
	if value := argumentValue("--addr"); value != "" {
		config.Addr = value
	}
	service, err := daemon.New(config)
	if err != nil {
		fmt.Fprintln(os.Stderr, "OpenADE daemon:", err)
		os.Exit(1)
	}
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	if err := service.Run(ctx); err != nil {
		fmt.Fprintln(os.Stderr, "OpenADE daemon:", err)
		os.Exit(1)
	}
}

func hasArgument(target string) bool {
	for _, arg := range os.Args[1:] {
		if arg == target {
			return true
		}
	}
	return false
}

func argumentValue(target string) string {
	for index, arg := range os.Args[1:] {
		if arg == target && index+2 <= len(os.Args)-1 {
			return os.Args[index+2]
		}
		prefix := target + "="
		if len(arg) > len(prefix) && arg[:len(prefix)] == prefix {
			return arg[len(prefix):]
		}
	}
	return ""
}
