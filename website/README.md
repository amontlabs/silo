# Silo website

The public site at [silo.amontlabs.com](https://silo.amontlabs.com), built with
Vite. It has two pages:

- **Home** (`index.html`, `src/site.css`, `src/style.css`): what Silo is, with
  links to the Workshop and the source code.
- **Workshop** (`workshop/index.html`, `src/workshop/`): Silo's plan, every
  feature in build order, with votes, comments, suggestions and the owner's
  verdicts. Its API lives under `/api/workshop/` (`server/`, `api/`); see
  [the Workshop](../docs/WORKSHOP.md).

Shared files: `public/theme.js` (first-paint light, dark or System theme,
remembered in local storage), `public/fonts/` (self-hosted IBM Plex under the
included SIL Open Font License) and `public/favicon.svg`.

## Run

Use Node.js 26. From the repository root:

```sh
npm --prefix website ci
npm --prefix website run dev
```

The page preview is at http://127.0.0.1:4173. The Workshop there has no API.
To run the site with its API against a fake GitHub:

```sh
npm --prefix website run build
npm --prefix website run workshop:dev -- --fake
```

Then open http://127.0.0.1:4174/workshop and sign in as `owner-fake` (can
decide) or `visitor-fake`. [Running locally](../docs/WORKSHOP.md#running-locally)
describes the fake and how to use a real development App instead.

## Test

```sh
npm --prefix website run typecheck
npm --prefix website test
```

`npm test` runs the Node tests in `scripts/*.test.mjs` (the theme script, and
the Workshop server against its fake GitHub, including the Vercel output
layout and the local server), then the page's Vitest tests.

## Build and deploy

```sh
npm --prefix website run build          # the static site, in website/dist
npm --prefix website run build:vercel   # the site and the API, in .vercel/output
```

`build:vercel` writes Vercel's Build Output (static files, the Workshop
function and its routes) to `.vercel/output` at the repository root, where a
Vercel build whose root directory is the repository reads it.
[Deployment](../docs/WORKSHOP.md#deployment) describes the layout.

The Vercel `silo` project is connected to `amontlabs/silo`. Pushes and merges to
`main` deploy production; other branches get preview deployments without the
Workshop's secrets. The project uses Node.js 26, the repository root as its
root directory, and the build command `npm --prefix website run build:vercel`.

For a manual deployment from the repository root:

```sh
npm --prefix website test
npm --prefix website run build:vercel
npx --yes vercel@59.16.0 deploy --prebuilt --prod --yes
```

For a new checkout, first run `npx --yes vercel@59.16.0 link --yes --project silo`.
The prebuilt deployment uploads only `.vercel/output/`, which Git ignores along
with Vercel's project metadata and local environment files.
