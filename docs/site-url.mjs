// The one place the published site's address is resolved.
//
// SITE_URL names where the built site is served, including its base path. The Astro
// config, the site build and the link check all read it here, so the Lab's mount and
// the header's Lab link derive from one value instead of being typed in two places.
export const DEFAULT_SITE_URL = 'https://velvetmonkey.github.io/safemesh/';
export const LAB_MOUNT = 'lab/';

export function siteUrl(value = process.env.SITE_URL) {
  const site = new URL(value || DEFAULT_SITE_URL);
  const path = site.pathname;
  if (!['http:', 'https:'].includes(site.protocol) || site.search || site.hash || site.username ||
      !path.endsWith('/') || path.includes('//') ||
      path.split('/').some((part) => part === '.' || part === '..') || decodeURI(path) !== path) {
    throw new Error(`SITE_URL must be an absolute HTTP(S) site address with a plain base path ending in /, not ${JSON.stringify(value)}`);
  }
  return site;
}

export function labUrl(site = siteUrl()) {
  return new URL(LAB_MOUNT, site);
}
