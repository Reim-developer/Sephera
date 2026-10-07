package main

import (
    "fmt"
    "os"

    "example.com/acme/internal/store"
    st "example.com/acme/internal/store"
    _ "example.com/acme/internal/metrics"
    . "example.com/acme/internal/flags"
)

func main() { fmt.Println(os.Args) }

