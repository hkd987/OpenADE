package daemon

import (
	"bufio"
	"encoding/json"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"time"
)

const maxProjectScanDepth = 4

type ExternalConversation struct {
	ID          string    `json:"id"`
	Provider    string    `json:"provider"`
	Title       string    `json:"title"`
	Cwd         string    `json:"cwd"`
	ProjectRoot string    `json:"project_root"`
	UpdatedAt   time.Time `json:"updated_at"`
}

func scanProjectRoot(root string) ([]string, error) {
	root = strings.TrimSpace(root)
	if root == "" {
		return nil, fmt.Errorf("project root is required")
	}
	abs, err := filepath.Abs(root)
	if err != nil {
		return nil, err
	}
	info, err := os.Stat(abs)
	if err != nil {
		return nil, fmt.Errorf("open project root: %w", err)
	}
	if !info.IsDir() {
		return nil, fmt.Errorf("project root must be a directory")
	}

	projects := []string{}
	err = filepath.WalkDir(abs, func(path string, entry fs.DirEntry, walkErr error) error {
		if walkErr != nil {
			if entry != nil && entry.IsDir() {
				return filepath.SkipDir
			}
			return nil
		}
		if !entry.IsDir() {
			return nil
		}
		rel, _ := filepath.Rel(abs, path)
		depth := 0
		if rel != "." {
			depth = len(strings.Split(rel, string(os.PathSeparator)))
		}
		if depth > maxProjectScanDepth {
			return filepath.SkipDir
		}
		if path != abs && shouldSkipProjectDir(entry.Name()) {
			return filepath.SkipDir
		}
		if _, statErr := os.Stat(filepath.Join(path, ".git")); statErr == nil {
			projects = append(projects, filepath.Clean(path))
			return filepath.SkipDir
		}
		return nil
	})
	if err != nil {
		return nil, err
	}
	sort.Strings(projects)
	return projects, nil
}

func discoverExternalConversations(root string, projects []string) []ExternalConversation {
	home, _ := os.UserHomeDir()
	type candidate struct {
		path, provider string
		updated        time.Time
	}
	candidates := []candidate{}
	for _, source := range []struct{ dir, provider string }{
		{filepath.Join(home, ".codex", "sessions"), "codex"},
		{filepath.Join(home, ".claude", "projects"), "claude"},
	} {
		_ = filepath.WalkDir(source.dir, func(path string, entry fs.DirEntry, err error) error {
			if err != nil {
				return nil
			}
			if entry.IsDir() {
				return nil
			}
			if filepath.Ext(path) != ".jsonl" {
				return nil
			}
			info, infoErr := entry.Info()
			if infoErr == nil {
				candidates = append(candidates, candidate{path: path, provider: source.provider, updated: info.ModTime().UTC()})
			}
			return nil
		})
	}
	sort.Slice(candidates, func(i, j int) bool { return candidates[i].updated.After(candidates[j].updated) })
	if len(candidates) > 600 {
		candidates = candidates[:600]
	}
	conversations := []ExternalConversation{}
	seen := map[string]bool{}
	for _, item := range candidates {
		conversation, ok := readExternalConversation(item.path, item.provider, item.updated)
		if !ok || !pathWithin(root, conversation.Cwd) {
			continue
		}
		conversation.ProjectRoot = matchingProject(conversation.Cwd, projects)
		if conversation.ProjectRoot == "" {
			continue
		}
		key := conversation.Provider + ":" + conversation.ID
		if seen[key] {
			continue
		}
		seen[key] = true
		conversations = append(conversations, conversation)
		if len(conversations) >= 100 {
			break
		}
	}
	return conversations
}

func readExternalConversation(path, provider string, updated time.Time) (ExternalConversation, bool) {
	file, err := os.Open(path)
	if err != nil {
		return ExternalConversation{}, false
	}
	defer file.Close()
	conversation := ExternalConversation{Provider: provider, UpdatedAt: updated}
	scanner := bufio.NewScanner(file)
	scanner.Buffer(make([]byte, 64*1024), 2*1024*1024)
	for scanner.Scan() {
		var event map[string]any
		if json.Unmarshal(scanner.Bytes(), &event) != nil {
			continue
		}
		if provider == "codex" {
			parseCodexConversationEvent(event, &conversation)
		} else {
			parseClaudeConversationEvent(event, &conversation)
		}
		if conversation.ID != "" && conversation.Cwd != "" && conversation.Title != "" {
			return conversation, true
		}
	}
	if conversation.Title == "" {
		conversation.Title = "Previous conversation"
	}
	return conversation, conversation.ID != "" && conversation.Cwd != ""
}

func parseCodexConversationEvent(event map[string]any, conversation *ExternalConversation) {
	payload, _ := event["payload"].(map[string]any)
	if event["type"] == "session_meta" {
		conversation.ID, _ = payload["id"].(string)
		conversation.Cwd, _ = payload["cwd"].(string)
	}
	if conversation.Title == "" && payload["type"] == "user_message" {
		conversation.Title = cleanConversationTitle(stringValue(payload["message"]))
	}
	if conversation.Title == "" && event["type"] == "response_item" && payload["role"] == "user" {
		conversation.Title = cleanConversationTitle(contentText(payload["content"]))
	}
}

func parseClaudeConversationEvent(event map[string]any, conversation *ExternalConversation) {
	if id, ok := event["sessionId"].(string); ok && id != "" {
		conversation.ID = id
	}
	if cwd, ok := event["cwd"].(string); ok && cwd != "" {
		conversation.Cwd = cwd
	}
	if conversation.Title == "" && event["type"] == "user" {
		message, _ := event["message"].(map[string]any)
		conversation.Title = cleanConversationTitle(contentText(message["content"]))
	}
}

func contentText(value any) string {
	if text, ok := value.(string); ok {
		return text
	}
	blocks, _ := value.([]any)
	for _, value := range blocks {
		block, _ := value.(map[string]any)
		if block["type"] == "input_text" || block["type"] == "text" {
			if text, ok := block["text"].(string); ok && text != "" {
				return text
			}
		}
	}
	return ""
}

func stringValue(value any) string {
	text, _ := value.(string)
	return text
}

func cleanConversationTitle(value string) string {
	value = strings.TrimSpace(strings.ReplaceAll(value, "\n", " "))
	value = strings.Join(strings.Fields(value), " ")
	if strings.HasPrefix(value, "<") || value == "" {
		return "Previous conversation"
	}
	if len(value) > 84 {
		return value[:81] + "…"
	}
	return value
}

func pathWithin(root, path string) bool {
	rel, err := filepath.Rel(filepath.Clean(root), filepath.Clean(path))
	return err == nil && rel != ".." && !strings.HasPrefix(rel, ".."+string(os.PathSeparator))
}

func matchingProject(cwd string, projects []string) string {
	best := ""
	for _, project := range projects {
		if pathWithin(project, cwd) && len(project) > len(best) {
			best = project
		}
	}
	return best
}

func shouldSkipProjectDir(name string) bool {
	if strings.HasPrefix(name, ".") {
		return true
	}
	switch name {
	case "node_modules", "vendor", "dist", "build", "DerivedData", "Library", "Pods", "target":
		return true
	default:
		return false
	}
}
