//go:build wasip1

package main

import (
	"encoding/json"
	"os"
)

type Input struct {
	// TODO: add your input fields
}

type Output struct {
	OK bool `json:"ok"`
}

func main() {
	var input Input
	if err := json.NewDecoder(os.Stdin).Decode(&input); err != nil {
		os.Exit(1)
	}
	out := Output{OK: true}
	if err := json.NewEncoder(os.Stdout).Encode(out); err != nil {
		os.Exit(1)
	}
}
