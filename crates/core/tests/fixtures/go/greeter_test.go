package greet

import "testing"

func TestGreet(t *testing.T) {
	g := &Greeter{prefix: "hi "}
	if got, _ := g.Greet("a"); got != "hi a" {
		t.Fatalf("got %q", got)
	}
}

func helper() {}

func TestAsserts(t *testing.T) {
	assert.Equal(t, 1, 1)
	require.NoError(t, nil)
}
