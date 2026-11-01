from typing import TypedDict

class _BookInfo(TypedDict):
    title: str
    authors: list[str]
    language: str
    chapters: int
    toc_entries: int

def convert(data: bytes, from_format: str, to_format: str) -> bytes: ...
def book_info(data: bytes, format: str) -> _BookInfo: ...
