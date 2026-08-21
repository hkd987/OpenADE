package daemon

import (
	"bufio"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

type AgentCommand struct {
	ID          string `json:"id"`
	Name        string `json:"name"`
	Kind        string `json:"kind"`
	Source      string `json:"source"`
	Invocation  string `json:"invocation"`
	Description string `json:"description,omitempty"`
}

func discoverAgentCommands(session Session) []AgentCommand {
	home, _ := os.UserHomeDir()
	agent := strings.ToLower(session.Agent)
	isClaude := strings.Contains(agent, "claude")
	candidates := []struct {
		root, pattern, kind, source string
	}{}
	if isClaude {
		candidates = append(candidates,
			struct{ root, pattern, kind, source string }{filepath.Join(home, ".claude", "commands"), "*.md", "command", "Claude"},
			struct{ root, pattern, kind, source string }{filepath.Join(home, ".claude", "skills"), "*/SKILL.md", "skill", "Claude"},
			struct{ root, pattern, kind, source string }{filepath.Join(session.RepoRoot, ".claude", "commands"), "*.md", "command", "Project"},
			struct{ root, pattern, kind, source string }{filepath.Join(session.RepoRoot, ".claude", "skills"), "*/SKILL.md", "skill", "Project"},
		)
	} else {
		candidates = append(candidates,
			struct{ root, pattern, kind, source string }{filepath.Join(home, ".codex", "skills"), "*/SKILL.md", "skill", "Codex"},
			struct{ root, pattern, kind, source string }{filepath.Join(home, ".agents", "skills"), "*/SKILL.md", "skill", "Shared"},
			struct{ root, pattern, kind, source string }{filepath.Join(session.RepoRoot, ".agents", "skills"), "*/SKILL.md", "skill", "Project"},
		)
	}

	seen := map[string]bool{}
	commands := []AgentCommand{}
	for _, candidate := range candidates {
		matches, _ := filepath.Glob(filepath.Join(candidate.root, candidate.pattern))
		for _, path := range matches {
			name := strings.TrimSuffix(filepath.Base(path), filepath.Ext(path))
			if filepath.Base(path) == "SKILL.md" {
				name = filepath.Base(filepath.Dir(path))
			}
			key := candidate.kind + ":" + name
			if name == "" || seen[key] {
				continue
			}
			seen[key] = true
			invocation := "$" + name
			if isClaude || candidate.kind == "command" {
				invocation = "/" + name
			}
			commands = append(commands, AgentCommand{
				ID: key, Name: name, Kind: candidate.kind, Source: candidate.source,
				Invocation: invocation, Description: commandDescription(path),
			})
		}
	}
	sort.Slice(commands, func(i, j int) bool {
		if commands[i].Kind == commands[j].Kind {
			return commands[i].Name < commands[j].Name
		}
		return commands[i].Kind < commands[j].Kind
	})
	if len(commands) > 120 {
		commands = commands[:120]
	}
	return commands
}

func commandDescription(path string) string {
	file, err := os.Open(path)
	if err != nil {
		return ""
	}
	defer file.Close()
	scanner := bufio.NewScanner(file)
	inFrontmatter := false
	frontmatterSeen := false
	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		if line == "---" {
			if !frontmatterSeen {
				frontmatterSeen = true
				inFrontmatter = true
			} else if inFrontmatter {
				inFrontmatter = false
			}
			continue
		}
		if strings.HasPrefix(line, "description:") {
			return strings.Trim(strings.TrimSpace(strings.TrimPrefix(line, "description:")), `"'`)
		}
		if !inFrontmatter && line != "" && !strings.HasPrefix(line, "#") {
			return strings.TrimSpace(line)
		}
	}
	return ""
}
