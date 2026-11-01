package greet

type Hook struct{}

func (h *Hook) Run() {}

type (
	Alpha struct{}
	Beta  struct{}
)

func (a Alpha) A() {}
func (b *Beta) B() {}
