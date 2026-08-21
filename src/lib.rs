#![no_std]
extern crate alloc;

use aidoku::{
	AidokuError, Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterValue, Home,
	HomeComponent, HomeComponentValue, HomeLayout, Link, Listing, ListingProvider, Manga,
	MangaPageResult, MangaStatus, Page, PageContent, Result, Source,
	alloc::{String, Vec, string::ToString},
	imports::net::Request,
	prelude::*,
};
use serde::Deserialize;

const BASE_URL: &str = "https://chikari.moe";
const API_URL: &str = "https://chikari.moe/api";

// ---------------------------------------------------------------------------
// JSON response shapes (confirmed against the live API)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ApiHomeResponse {
	rows: Vec<ApiHomeRow>,
}

#[derive(Deserialize)]
struct ApiHomeRow {
	slug: String,
	items: Vec<ApiSeriesCard>,
}

#[derive(Deserialize)]
struct ApiSeriesCard {
	slug: String,
	title: String,
	cover_url: String,
	status: String,
	#[serde(default)]
	is_nsfw: bool,
}

#[derive(Deserialize)]
struct ApiSeriesDetails {
	title: String,
	status: String,
	#[serde(default)]
	is_nsfw: bool,
	description: String,
	cover_url: String,
	#[serde(default)]
	genres: Vec<ApiNamedTag>,
	#[serde(default)]
	tags: Vec<ApiNamedTag>,
	#[serde(default)]
	authors: Vec<ApiAuthor>,
}

#[derive(Deserialize)]
struct ApiNamedTag {
	name: String,
}

#[derive(Deserialize)]
struct ApiAuthor {
	name: String,
	role: String,
}

#[derive(Deserialize)]
struct ApiChapterListResponse {
	items: Vec<ApiChapterListItem>,
	total: i64,
}

#[derive(Deserialize)]
struct ApiChapterListItem {
	number: f32,
	#[serde(default)]
	title: String,
}

#[derive(Deserialize)]
struct ApiChapterPagesResponse {
	pages: Vec<String>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fetch_json<T: for<'de> Deserialize<'de>>(url: &str) -> Result<T> {
	let body = Request::get(url)
		.map_err(|_| AidokuError::message("invalid request url"))?
		.send()
		.map_err(|_| AidokuError::message("request failed"))?
		.get_string()
		.map_err(|_| AidokuError::message("invalid response body"))?;
	serde_json::from_str(&body).map_err(|_| AidokuError::message("failed to parse json"))
}

fn status_from_str(status: &str) -> MangaStatus {
	match status {
		"releasing" => MangaStatus::Ongoing,
		"completed" => MangaStatus::Completed,
		"hiatus" => MangaStatus::Hiatus,
		"cancelled" => MangaStatus::Cancelled,
		_ => MangaStatus::Unknown,
	}
}

fn content_rating_from_nsfw(is_nsfw: bool) -> ContentRating {
	if is_nsfw { ContentRating::NSFW } else { ContentRating::Safe }
}

fn manga_from_card(slug: &str, card: ApiSeriesCard) -> Manga {
	Manga {
		key: slug.to_string(),
		title: card.title,
		cover: Some(card.cover_url),
		url: Some(alloc::format!("{BASE_URL}/series/{slug}")),
		status: status_from_str(&card.status),
		content_rating: content_rating_from_nsfw(card.is_nsfw),
		..Default::default()
	}
}

// Format a chapter number for display/URLs: "2" instead of "2.0", but "115.5" stays as-is.
fn format_chapter_number(n: f32) -> String {
	let as_int = n as i64;
	if (n - as_int as f32).abs() < 0.01 {
		alloc::format!("{}", as_int)
	} else {
		alloc::format!("{n}")
	}
}

// ---------------------------------------------------------------------------
// Source
// ---------------------------------------------------------------------------

struct Chikari;

impl Source for Chikari {
	fn new() -> Self {
		Self
	}

	// Chikari's live search API endpoint (query param name, pagination) is not
	// yet confirmed. Once confirmed, replace the body below with a real call
	// to something like: {API_URL}/search?q={query}&page={page}
	fn get_search_manga_list(
		&self,
		query: Option<String>,
		_page: i32,
		_filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		let _ = query;
		Err(AidokuError::Unimplemented)
	}

	// Called to fetch/refresh a manga's details and/or chapter list.
	fn get_manga_update(
		&self,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		let slug = manga.key.clone();

		if needs_details {
			let details: ApiSeriesDetails = fetch_json(&alloc::format!("{API_URL}/series/{slug}"))?;

			let mut tags: Vec<String> = details.genres.into_iter().map(|g| g.name).collect();
			tags.extend(details.tags.into_iter().map(|t| t.name));

			let authors: Vec<String> = details
				.authors
				.iter()
				.filter(|a| a.role == "author")
				.map(|a| a.name.clone())
				.collect();
			let artists: Vec<String> = details
				.authors
				.iter()
				.filter(|a| a.role == "artist")
				.map(|a| a.name.clone())
				.collect();

			manga.title = details.title;
			manga.cover = Some(details.cover_url);
			manga.description = Some(details.description);
			manga.url = Some(alloc::format!("{BASE_URL}/series/{slug}"));
			manga.tags = Some(tags);
			manga.authors = Some(authors);
			manga.artists = Some(artists);
			manga.status = status_from_str(&details.status);
			manga.content_rating = content_rating_from_nsfw(details.is_nsfw);
		}

		if needs_chapters {
			let mut all_chapters = Vec::new();
			let mut offset = 0i64;
			let limit = 100i64;

			loop {
				let url = alloc::format!(
					"{API_URL}/series/{slug}/chapters?order=desc&limit={limit}&offset={offset}"
				);
				let page: ApiChapterListResponse = fetch_json(&url)?;
				let total = page.total;

				for item in page.items {
					let number = item.number;
					let number_str = format_chapter_number(number);
					all_chapters.push(Chapter {
						key: number_str.clone(),
						title: if item.title.is_empty() { None } else { Some(item.title) },
						chapter_number: Some(number),
						url: Some(alloc::format!("{BASE_URL}/series/{slug}/{number_str}")),
						..Default::default()
					});
				}

				offset += limit;
				if offset >= total {
					break;
				}
			}

			manga.chapters = Some(all_chapters);
		}

		Ok(manga)
	}

	fn get_page_list(&self, manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let slug = manga.key;
		let number = chapter.key;
		let url = alloc::format!("{API_URL}/series/{slug}/chapters/{number}");
		let data: ApiChapterPagesResponse = fetch_json(&url)?;

		Ok(data
			.pages
			.into_iter()
			.map(|page_url| Page {
				content: PageContent::url(page_url),
				..Default::default()
			})
			.collect())
	}
}

// ---------------------------------------------------------------------------
// ListingProvider (browse tabs backed by the home feed rows)
// ---------------------------------------------------------------------------

impl ListingProvider for Chikari {
	fn get_manga_list(&self, listing: Listing, _page: i32) -> Result<MangaPageResult> {
		let url = alloc::format!(
			"{API_URL}/home?content_rating=safe%2Csuggestive&type=manga&type=manhwa&type=manhua&type=oel"
		);
		let home: ApiHomeResponse = fetch_json(&url)?;

		let row = home
			.rows
			.into_iter()
			.find(|r| r.slug == listing.id)
			.ok_or(AidokuError::message("listing not found"))?;

		let entries = row
			.items
			.into_iter()
			.map(|card| manga_from_card(&card.slug.clone(), card))
			.collect();

		// The home feed isn't paginated per row -- it's a fixed batch, so there's no next page.
		Ok(MangaPageResult { entries, has_next_page: false })
	}
}

// ---------------------------------------------------------------------------
// Home (default browse screen)
// ---------------------------------------------------------------------------

impl Home for Chikari {
	fn get_home(&self) -> Result<HomeLayout> {
		let url = alloc::format!(
			"{API_URL}/home?content_rating=safe%2Csuggestive&type=manga&type=manhwa&type=manhua&type=oel"
		);
		let home: ApiHomeResponse = fetch_json(&url)?;

		let row_titles = [
			("trending", "Trending"),
			("popular", "Popular"),
			("top-rated", "Top Rated"),
			("recently-updated", "Recently Updated"),
			("recently-added", "Recently Added"),
		];

		let mut components = Vec::new();

		for row in home.rows {
			let title = row_titles
				.iter()
				.find(|(slug, _)| *slug == row.slug)
				.map(|(_, title)| *title)
				.unwrap_or("Row");

			let links: Vec<Link> = row
				.items
				.into_iter()
				.map(|card| Link::from(manga_from_card(&card.slug.clone(), card)))
				.collect();

			components.push(HomeComponent {
				title: Some(title.to_string()),
				subtitle: None,
				value: HomeComponentValue::Scroller { entries: links, listing: None },
			});
		}

		Ok(HomeLayout { components })
	}
}

// ---------------------------------------------------------------------------
// DeepLinkHandler (opening chikari.moe links inside the app)
// ---------------------------------------------------------------------------

impl DeepLinkHandler for Chikari {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let path = url.trim_start_matches(BASE_URL);
		let parts: Vec<&str> = path.trim_matches('/').split('/').collect();

		// /series/{slug} -> manga page
		// /series/{slug}/{chapter} -> chapter page
		if parts.len() >= 2 && parts[0] == "series" {
			let slug = parts[1].to_string();
			if parts.len() >= 3 {
				return Ok(Some(DeepLinkResult::Chapter {
					manga_key: slug,
					key: parts[2].to_string(),
				}));
			}
			return Ok(Some(DeepLinkResult::Manga { key: slug }));
		}

		Ok(None)
	}
}

register_source!(Chikari, ListingProvider, Home, DeepLinkHandler);
