// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { CodeSource } from "../../api";
import { CodeSources, knownTokenHost, sourceStatus } from "./CodeSources";

const folder: CodeSource = {
  id: "src_web",
  label: "web",
  kind: { type: "local", path: "/Users/me/code/web" },
  lastFetch: null,
  lastCommit: null,
  error: null,
};
const repo: CodeSource = {
  id: "src_api",
  label: "api",
  kind: { type: "git", url: "https://github.com/acme/api.git", branch: "main", subdir: null, hasToken: true, tokenUser: null },
  lastFetch: Date.now() - 2 * 3_600_000,
  lastCommit: "1a2b3c4d5e6f",
  error: null,
};

let sources: CodeSource[] = [];
type Repo = { url: string; branch: string; subdir: string; token: string | null; tokenUser: string | null };
const addCodeRepo = vi.fn((_id: string, r: Repo) =>
  Promise.resolve<CodeSource>({
    id: "src_new",
    label: "shop",
    kind: { type: "git", url: r.url, branch: r.branch, subdir: r.subdir || null, hasToken: !!r.token, tokenUser: r.tokenUser },
    lastFetch: null,
    lastCommit: null,
    error: null,
  }),
);
const removeCodeSource = vi.fn((_id: string, _source: string) => Promise.resolve());
const checkCodeSource = vi.fn((_id: string, source: string) =>
  Promise.resolve({ ...repo, id: source, error: "github.com didn't accept the sign-in. Add a read-only token for this repository." }),
);
const addCodeFolder = vi.fn((_id: string) => Promise.resolve<CodeSource | null>(null));

vi.mock("../../api", async (orig) => ({
  ...(await orig<typeof import("../../api")>()),
  commands: {
    codeSources: () => Promise.resolve(sources),
    addCodeRepo: (id: string, r: Repo) => addCodeRepo(id, r),
    removeCodeSource: (id: string, s: string) => removeCodeSource(id, s),
    checkCodeSource: (id: string, s: string) => checkCodeSource(id, s),
    addCodeFolder: (id: string) => addCodeFolder(id),
  },
}));

beforeEach(() => {
  sources = [folder, repo];
  addCodeRepo.mockClear();
  removeCodeSource.mockClear();
  checkCodeSource.mockClear();
});
afterEach(cleanup);

it("lists each source with where it is and how its last fetch went", async () => {
  const onChange = vi.fn();
  render(<CodeSources connectionId="c1" onChange={onChange} />);
  const list = await screen.findByRole("list", { name: "Code sources" });
  const items = within(list).getAllByRole("listitem");
  expect(items).toHaveLength(2);
  expect(items[0].textContent).toContain("/Users/me/code/web");
  expect(items[0].textContent).toContain("Read where it is");
  expect(items[1].textContent).toContain("https://github.com/acme/api.git · main");
  expect(items[1].textContent).toContain("Fetched 2 hours ago · 1a2b3c4 · token saved");
  expect(onChange).toHaveBeenLastCalledWith([folder, repo]);
});

it("removes a source and checks access, showing the error", async () => {
  const onChange = vi.fn();
  render(<CodeSources connectionId="c1" onChange={onChange} />);
  fireEvent.click(await screen.findByRole("button", { name: "Check access to api" }));
  expect(await screen.findByText(/didn't accept the sign-in/)).toBeTruthy();
  expect(checkCodeSource).toHaveBeenCalledWith("c1", "src_api");
  fireEvent.click(screen.getByRole("button", { name: "Remove web" }));
  await waitFor(() => expect(removeCodeSource).toHaveBeenCalledWith("c1", "src_web"));
  await waitFor(() => expect(screen.queryByText("/Users/me/code/web")).toBeNull());
  expect(onChange.mock.lastCall?.[0].map((s: CodeSource) => s.id)).toEqual(["src_api"]);
});

it("adds a repository with a branch, a folder and a token", async () => {
  sources = [];
  render(<CodeSources connectionId="c1" />);
  expect(await screen.findByText(/No code yet/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Add repository…" }));
  const form = screen.getByRole("form", { name: "Add a repository" });
  const add = within(form).getByRole("button", { name: "Add repository" }) as HTMLButtonElement;
  expect(add.disabled).toBe(true);
  fireEvent.change(within(form).getByLabelText("Repository address"), { target: { value: " https://github.com/acme/shop.git " } });
  fireEvent.change(within(form).getByLabelText(/Branch/), { target: { value: "release" } });
  fireEvent.change(within(form).getByLabelText(/Folder in the repository/), { target: { value: "services/api" } });
  const token = within(form).getByLabelText(/Read-only token/) as HTMLInputElement;
  expect(token.type).toBe("password");
  fireEvent.change(token, { target: { value: "ghp_x" } });
  // GitHub's token user name is known: nothing more to ask.
  expect(within(form).queryByLabelText("User name for the token")).toBeNull();
  fireEvent.click(add);
  await waitFor(() =>
    expect(addCodeRepo).toHaveBeenCalledWith("c1", {
      url: "https://github.com/acme/shop.git",
      branch: "release",
      subdir: "services/api",
      token: "ghp_x",
      tokenUser: null,
    }),
  );
  expect(await screen.findByText("https://github.com/acme/shop.git · release · services/api")).toBeTruthy();
  expect(screen.queryByRole("form", { name: "Add a repository" })).toBeNull();
});

it("asks for the token's user name on a server it doesn't know, and shows why adding failed", async () => {
  sources = [];
  addCodeRepo.mockImplementationOnce(() => Promise.reject("git.acme.dev didn't accept the sign-in."));
  render(<CodeSources connectionId="c1" />);
  fireEvent.click(await screen.findByRole("button", { name: "Add repository…" }));
  const form = screen.getByRole("form", { name: "Add a repository" });
  fireEvent.change(within(form).getByLabelText("Repository address"), { target: { value: "https://git.acme.dev/shop.git" } });
  fireEvent.change(within(form).getByLabelText(/Read-only token/), { target: { value: "tok" } });
  const add = within(form).getByRole("button", { name: "Add repository" }) as HTMLButtonElement;
  expect(add.disabled).toBe(true);
  fireEvent.change(within(form).getByLabelText("User name for the token"), { target: { value: "deploy" } });
  fireEvent.click(add);
  expect(await within(form).findByText("git.acme.dev didn't accept the sign-in.")).toBeTruthy();
  expect(addCodeRepo.mock.lastCall?.[1].tokenUser).toBe("deploy");
  fireEvent.click(within(form).getByRole("button", { name: "Cancel" }));
  expect(screen.queryByRole("form", { name: "Add a repository" })).toBeNull();
});

it("knows the token user names of GitHub, GitLab and Bitbucket", () => {
  expect(knownTokenHost("https://github.com/a/b")).toBe(true);
  expect(knownTokenHost("https://gitlab.acme.dev/a/b")).toBe(true);
  expect(knownTokenHost("https://bitbucket.org/a/b")).toBe(true);
  expect(knownTokenHost("https://git.acme.dev/a/b")).toBe(false);
  expect(sourceStatus({ ...repo, lastFetch: null })).toBe("Fetched when the audit runs");
  expect(sourceStatus({ ...folder, lastFetch: 1000, lastCommit: "abcdef123" }, 1000 + 3 * 86_400_000)).toBe("Read 3 days ago · abcdef1");
});
