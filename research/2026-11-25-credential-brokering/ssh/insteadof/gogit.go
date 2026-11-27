package main

import (
	"fmt"
	"os"

	git "github.com/go-git/go-git/v5"
)

func main() {
	_, err := git.PlainClone(os.Args[1], false, &git.CloneOptions{URL: "git@github.com:octocat/Hello-World.git", Depth: 1})
	fmt.Println("err:", err)
}
