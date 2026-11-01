// Package greet says hello.
package greet

import "fmt"

// Greeter says hello.
type Greeter struct {
	prefix string
}

type Namer interface {
	Name() string
}

func (g *Greeter) Greet(name string) (string, error) {
	text := g.prefix + name
	fmt.Println("greeting", name)
	out, err := render(text)
	if err != nil {
		fmt.Println(err)
		return "", err
	}
	return out, nil
}

func render(text string) (string, error) {
	return text, nil
}

func (g Greeter) Shout(name string) string {
	switch name {
	case "a", "b":
		return "A"
	default:
		return name
	}
}
