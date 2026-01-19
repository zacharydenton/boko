import io
import os
from PIL import Image
import subprocess
import time

from .jxr_container import JXRContainer
from .message_logging import log
from .utilities import (
    add_plugin_path, calibre_numeric_version, create_temp_dir, disable_debug_log, natural_sort_key,
    remove_plugin_path, temp_filename)
from .utilities import IS_WINDOWS

if calibre_numeric_version is not None:
    add_plugin_path()
    import pypdf
    remove_plugin_path()
else:
    import pypdf


__license__ = "GPL v3"
__copyright__ = "2016-2025, John Howell <jhowell@acm.org>"

COMBINE_TILES_LOSSLESS = True
MIN_JPEG_QUALITY = 90
MAX_JPEG_QUALITY = 100
COMBINED_TILE_SIZE_FACTOR = 1.2
TILE_SIZE_REPORT_PERCENTAGE = 10
DEBUG_TILES = False
DEBUG_PDF_IMAGE_EXTRACTION = False
PDF_TO_IMAGE_DPI = 300

CONVERT_JXR_LOSSLESS = False

IMAGE_COLOR_MODES = [
    "1",
    "L",
    "P",
    "RGB",
    ]

IMAGE_OPACITY_MODE = "A"


FORMAT_SYMBOLS = {
    "bmp": "$599",
    "gif": "$286",
    "jpg": "$285",
    "jxr": "$548",
    "pbm": "$420",
    "pdf": "$565",
    "png": "$284",
    "pobject": "$287",
    "tiff": "$600",
    "bpg": "$612",
    }

SYMBOL_FORMATS = {}
for k, v in FORMAT_SYMBOLS.items():
    SYMBOL_FORMATS[v] = k


MIMETYPE_OF_EXT = {
    ".aax": "audio/vnd.audible.aax",
    ".apnx": "application/x-apnx-sidecar",
    ".bin": "application/octet-stream",
    ".bmp": "image/bmp",
    ".css": "text/css",
    ".eot": "application/vnd.ms-fontobject",
    ".dfont": "application/x-dfont",
    ".epub": "application/epub+zip",
    ".gif": "image/gif",
    ".htm": "text/html",
    ".html": "text/html",
    ".ico": "image/x-icon",
    ".jpeg": "image/jpeg",
    ".jpg": "image/jpeg",
    ".js": "text/javascript",
    ".jxr": "image/vnd.ms-photo",
    ".kvg": "image/kvg",
    ".mp3": "audio/mpeg",
    ".mp4": "video/mp4",
    ".mpg": "video/mpeg",
    ".ncx": "application/x-dtbncx+xml",
    ".opf": "application/oebps-package+xml",
    ".otf": "font/otf",
    ".pfb": "application/x-font-type1",
    ".png": "image/png",
    ".pdf": "application/pdf",
    ".pobject": "application/azn-plugin-object",
    ".svg": "image/svg+xml",
    ".tif": "image/tiff",
    ".tiff": "image/tiff",
    ".ttf": "font/ttf",
    ".txt": "text/plain",
    ".webp": "image/webp",
    ".woff": "application/font-woff",
    ".woff2": "font/woff2",
    ".xhtml": "application/xhtml+xml",
    ".xml": "application/xml",
    }

EPUB2_ALT_MIMETYPES = {
    "font/ttf": "application/x-font-truetype",
    "font/otf": "application/x-font-otf",
    }

RESOURCE_TYPE_OF_EXT = {
    ".bmp": "image",
    ".css": "styles",
    ".eot": "font",
    ".dfont": "font",
    ".gif": "image",
    ".htm": "text",
    ".html": "text",
    ".ico": "image",
    ".jpg": "image",
    ".js": "text",
    ".jxr": "image",
    ".kvg": "image",
    ".mp3": "audio",
    ".mp4": "video",
    ".otf": "font",
    ".pdf": "image",
    ".png": "image",
    ".svg": "image",
    ".tif": "image",
    ".tiff": "image",
    ".ttf": "font",
    ".txt": "text",
    ".webp": "video",
    ".woff": "font",
    ".woff2": "font",
    }

EXTS_OF_MIMETYPE = {
    "application/azn-plugin-object": [".pobject"],
    "application/epub+zip": [".epub"],
    "application/font-sfnt": [".ttf", ".otf"],
    "application/font-woff": [".woff"],
    "application/font-woff2": [".woff2"],
    "application/javascript": [".js", ".jsonp", ".json"],
    "application/json": [".json"],
    "application/json+ea": [".json"],
    "application/json+xray": [".json"],
    "application/ocsp-response": [".ocsp"],
    "application/octet-stream": [".bin"],
    "application/oebps-package+xml": [".opf"],
    "application/pdf": [".pdf"],
    "application/protobuf": [".bin"],
    "application/sql+xray": [".db"],
    "application/vnd.adobe-page-template+xml": [".xpgt"],
    "application/vnd.amazon.ebook": [".azw"],
    "application/vnd.ms-fontobject": [".eot"],
    "application/vnd.ms-opentype": [".otf", ".ttf"],
    "application/vnd.ms-sync.wbxml": [".xml"],
    "application/x-amz-json-1.1": [".json"],
    "application/x-amzn-ion": [".ion"],
    "application/x-apnx-sidecar": [".apnx"],
    "application/x-bzip": [".bz"],
    "application/x-bzip2": [".bz2"],
    "application/x-dfont": [".dfont"],
    "application/x-dtbncx+xml": [".ncx"],
    "application/x-font-otf": [".otf"],
    "application/x-font-truetype": [".ttf"],
    "application/x-font-ttf": [".ttf"],
    "application/x-font-woff": [".woff"],
    "application/x-javascript": [".js"],
    "application/x-kfx-ebook": [".kfx", ".azw8", ".azw9"],
    "application/x-kfx-magazine": [".kfx"],
    "application/x-mobi8-ebook": [".azw3"],
    "application/x-mobi8-images": [".azw6"],
    "application/x-mobipocket-ebook-mop": [".azw4"],
    "application/x-font-type1": [".pfb"],
    "application/x-rar-compressed": [".rar"],
    "application/x-protobuf": [".bin"],
    "application/x-tar": [".tar"],
    "application/x-x509-ca-cert": [".der"],
    "application/xhtml+xml": [".xhtml", ".html", ".htm"],
    "application/xml": [".xml"],
    "application/xml+phl": [".xml"],
    "application/xml-dtd": [".dtd"],
    "application/xslt+xml": [".xslt"],
    "application/zip": [".zip"],
    "application/zip+mpub": [".zip"],
    "audio": [".mp3"],
    "audio/mp3": [".mp4"],
    "audio/mp4": [".mp4"],
    "audio/mpeg": [".mp3"],
    "audio/vnd.audible.aax": [".aax"],
    "figure": [".figure"],
    "font/otf": [".otf"],
    "font/ttf": [".ttf"],
    "font/woff": [".woff"],
    "font/woff2": [".woff2"],
    "image/bmp": [".bmp"],
    "image/gif": [".gif"],
    "image/jpeg": [".jpg", ".jpeg"],
    "image/jpg": [".jpg", ".jpeg"],
    "image/jxr": [".jxr"],
    "image/png": [".png"],
    "image/svg+xml": [".svg"],
    "image/tiff": [".tif", ".tiff"],
    "image/vnd.djvu": [".djvu"],
    "image/vnd.ms-photo": [".jxr"],
    "image/vnd.jxr": [".jxr"],
    "image/webp": [".webp"],
    "image/x-icon": [".ico"],
    "plugin/kfx-html-article": [".html"],
    "res/bin": [".bin"],
    "res/img": [".png"],
    "res/kvg": [".kvg"],
    "text/css": [".css"],
    "text/csv": [".csv"],
    "text/html": [".html", ".htm"],
    "text/json": [".json"],
    "text/javascript": [".js"],
    "text/plain": [".txt"],
    "text/xml": [".xml"],
    "video": [".mp4"],
    "video/h264": [".mp4"],
    "video/mp4": [".mp4"],
    "video/mpeg": [".mpg"],
    "video/ogg": [".ogg"],
    "video/webm": [".webm"],
    }


class ImageResource(object):
    def __init__(self, format, location, raw_media, height=None, width=None):
        self.format = format
        self.location = location
        self.raw_media = raw_media
        self.height = height
        self.width = width


class PdfImageResource(ImageResource):
    def __init__(self, location, raw_media, page_index, total_pages):
        ImageResource.__init__(self, "$565", location, raw_media)
        self.page_nums = [page_index + 1]
        self.total_pages = total_pages

    def entire_resource_used(self):
        return self.page_nums == list(range(1, self.total_pages + 1))

    def page_number_ranges(self):
        ranges = []
        start = end = None

        for page_num in self.page_nums:
            if start is None:
                start = end = page_num
            elif page_num == end + 1:
                end = page_num
            else:
                ranges.append((start, end + 1))
                start = end = page_num

        if start is not None:
            ranges.append((start, end + 1))

        return ranges


def convert_jxr_to_jpeg_or_png(jxr_data, resource_name, return_mime=False):
    try:
        image_data = convert_jxr_to_tiff(jxr_data, resource_name)
    except Exception as e:
        log.error("Exception during conversion of JPEG-XR '%s' to TIFF: %s" % (resource_name, repr(e)))
        image_data = jxr_data
        image_type = "$548"
    else:
        with disable_debug_log():
            img = Image.open(io.BytesIO(image_data))
            image_type, ofmt, optimize = ("$284", "PNG", False) if CONVERT_JXR_LOSSLESS or img.mode == "RGBA" else ("$285", "JPEG", True)
            outfile = io.BytesIO()
            img.save(outfile, ofmt, quality=95, optimize=optimize)
            img.close()

        image_data = outfile.getvalue()
        outfile.close()

    return image_data, MIMETYPE_OF_EXT["." + SYMBOL_FORMATS[image_type]] if return_mime else image_type


def convert_jxr_to_tiff(jxr_data, resource_name):

    if calibre_numeric_version is not None:

        try:
            from calibre.utils.img import (load_jxr_data, image_to_data)
            img = load_jxr_data(jxr_data)
            tiff_data = image_to_data(img, fmt="TIFF")

            if tiff_data:
                return tiff_data
        except Exception as e:
            log.warning("Conversion of JPEG-XR resource failed: %s" % repr(e))

        log.info("Using fallback JPEG-XR conversion for %s" % resource_name)

    start_time = time.time()

    im = JXRContainer(jxr_data).unpack_image()

    duration = time.time() - start_time
    if duration >= 5.0:
        log.info("JPEG-XR to TIFF conversion took %0.1f sec" % duration)

    outfile = io.BytesIO()
    with disable_debug_log():
        im.save(outfile, "TIFF")
        im.close()
        del im

    return outfile.getvalue()


def convert_pdf_page_to_image(location, pdf_data, page_num, reported_errors=None, force_jpeg=False):
    default_image = (convert_pdf_page_to_jpeg(location, pdf_data, page_num, reported_errors), "$285")
    return get_pdf_page_image(location, pdf_data, page_num, force_jpeg, default_image)


def convert_pdf_page_to_jpeg(location, pdf_data, page_num, reported_errors=None):
    pdf_file = temp_filename("pdf", pdf_data)
    jpeg_dir = create_temp_dir()

    if calibre_numeric_version is not None:

        if PDF_TO_IMAGE_DPI == 150:
            from calibre.ebooks.metadata.pdf import page_images
            page_images(pdf_file, jpeg_dir, first=page_num, last=page_num)
        else:
            from calibre.ebooks.metadata.pdf import get_tools
            pdftoppm = get_tools()[1]

            try:
                subprocess.check_call([
                        pdftoppm, "-cropbox", "-jpeg", "-r", "%d" % PDF_TO_IMAGE_DPI, "-f", str(page_num),
                        "-l", str(page_num), pdf_file, os.path.join(os.path.abspath(jpeg_dir), "page-images")],
                    creationflags=subprocess.DETACHED_PROCESS if IS_WINDOWS else 0)
            except subprocess.CalledProcessError as e:
                raise ValueError(f"Failed to render PDF, pdftoppm errorcode: {e.returncode}")

    for dirpath, dirnames, filenames in os.walk(jpeg_dir):
        if len(filenames) != 1:
            raise Exception("pdftoppm created %d files" % len(filenames))

        if not (filenames[0].endswith(".jpg") or filenames[0].endswith(".jpeg")):
            raise Exception("pdftoppm created unexpected file: %s" % filenames[0])

        with io.open(os.path.join(dirpath, filenames[0]), "rb") as of:
            jpeg_data = of.read()

        break
    else:
        raise Exception("pdftoppm  %s page %d created no files" % (location, page_num))

    return jpeg_data


def get_pdf_page_image(location, pdf_data, page_num, force_jpeg, default_image):

    try:
        with disable_debug_log():
            raw_media_file = io.BytesIO(pdf_data)
            pdf = pypdf.PdfReader(raw_media_file)
            page = pdf.pages[page_num - 1]

            if box_tuple(page.cropbox) != box_tuple(page.mediabox):
                return default_image

            if len(page.images.keys()) != 1:
                return default_image

            text = page.extract_text()
            if text:
                return default_image

            if "/Annots" in page:
                for annot in page["/Annots"]:
                    obj = annot.get_object()
                    if "/Contents" in obj:
                        return default_image

            page_image = page.images[0]
            image_object = page_image.indirect_reference.get_object()
            image_data = image_object.get_data()

            if image_object.get("/Type") != "/XObject" or image_object.get("/Subtype") != "/Image":
                return default_image

            media_width, media_height = box_size(page.mediabox)
            img_width = int(image_object.get("/Width"))
            img_height = int(image_object.get("/Height"))

            page_aspect = media_width / media_height
            image_aspect = img_width / img_height

            if abs(page_aspect - image_aspect) * page_aspect > 0.001:
                return default_image

            image_dpi = img_width / (media_width / 72.0)
            if image_dpi < 75.0:
                return default_image

            if ((image_object.get("/Filter") == "/DCTDecode" and image_file_ext(image_data) == ".jpg") or
                    ("/CCITTFaxDecode" in image_object.get("/Filter") and image_file_ext(image_data) == ".tif")):
                pil_img = Image.open(io.BytesIO(image_data))
            elif image_object.get("/Filter") == "/FlateDecode":
                pil_img = page_image.image
                image_data = page_image.data
            else:
                return default_image

            if pil_img.format == "TIFF":
                outfile = io.BytesIO()
                pil_img.save(outfile, "PNG", optimize=True)
                image_data = outfile.getvalue()
                outfile.close()
                pil_img = Image.open(io.BytesIO(image_data))

            if pil_img.format not in {"JPEG", "PNG"}:
                return default_image

            if pil_img.mode not in {"RGB", "RGBA", "L", "1"}:
                return default_image

            if force_jpeg and pil_img.format != "JPEG":
                if pil_img.mode == "RGBA":
                    pil_img = pil_img.convert("RGB")
                outfile = io.BytesIO()
                pil_img.save(outfile, "JPEG", quality=95, optimize=True)
                image_data = outfile.getvalue()
                outfile.close()

            fmt = FORMAT_SYMBOLS[pil_img.format.lower().replace("jpeg", "jpg")]

            if not image_match(pil_img, Image.open(io.BytesIO(default_image[0])), location, page_num):
                return default_image

            log.info("Extracting %s image (%dx%d mode %s) from PDF %s page %d" % (
                pil_img.format, img_width, img_height, pil_img.mode, location, page_num))
            return image_data, fmt

    except Exception as e:
        e       # prevent warning
        return default_image


def image_match(img1, img2, location, page_num):
    SCALE = 16
    MAX_DIFF_PER_PIXEL = 10
    REMOVE_MARGIN = 2

    if img1.mode != "RGB":
        img1 = img1.convert("RGB")

    if img2.mode != "RGB":
        img2 = img2.convert("RGB")

    width = min(img1.size[0], img2.size[0]) // SCALE
    height = min(img1.size[1], img2.size[1]) // SCALE
    new_size = (width, height)

    img1 = img1.resize(new_size, Image.BICUBIC)
    img2 = img2.resize(new_size, Image.BICUBIC)

    max_diff = MAX_DIFF_PER_PIXEL * width * height

    pix1 = img1.load()
    pix2 = img2.load()
    total = 0

    for y in range(REMOVE_MARGIN, height - REMOVE_MARGIN):
        for x in range(REMOVE_MARGIN, width - REMOVE_MARGIN):
            diff = (abs(pix1[x, y][0] - pix2[x, y][0]) + abs(pix1[x, y][1] - pix2[x, y][1]) + abs(pix1[x, y][2] - pix2[x, y][2]))

            if diff > 50:
                total += diff ** 2
                if total > max_diff and not DEBUG_PDF_IMAGE_EXTRACTION:
                    break

        if total > max_diff and not DEBUG_PDF_IMAGE_EXTRACTION:
            break

    if total > max_diff:
        return False

    return True


def convert_image_to_pdf(image_resource):
    if image_resource.format == "$565":
        return image_resource

    image_data = image_resource.raw_media

    if image_resource.format == "$548":
        image_data = convert_jxr_to_jpeg_or_png(image_data, image_resource.location)[0]

    pdf_file = io.BytesIO()

    with disable_debug_log():
        image = Image.open(io.BytesIO(image_data))
        if image.mode != "RGB" and natural_sort_key(Image.__version__) < natural_sort_key("9.5.0"):
            image = image.convert("RGB")

        image.save(pdf_file, "pdf", save_all=True)
        image.close()

    pdf_data = pdf_file.getvalue()
    pdf_file.close()

    return PdfImageResource(image_resource.location, pdf_data, 0, 1)


def combine_image_tiles(
        resource_name, resource_height, resource_width, resource_format, tile_height, tile_width, tile_padding,
        yj_tiles, tiles_raw_media, ignore_variants):

    if DEBUG_TILES:
        ncols = len(yj_tiles)
        nrows = len(yj_tiles[0])
        log.warning("tiled image %dx%d: %s" % (nrows, ncols, resource_name))

    with disable_debug_log():
        tile_images = []
        separate_tiles_size = tile_count = 0
        full_image_color_mode = IMAGE_COLOR_MODES[0]
        full_image_opacity_mode = ""

        tile_num = 0
        missing_tiles = []
        for y, row in enumerate(yj_tiles):
            for x, tile_location in enumerate(row):
                tile_raw_media = tiles_raw_media[tile_num]
                if tile_raw_media is not None:
                    tile_count += 1
                    separate_tiles_size += len(tile_raw_media)
                    tile = Image.open(io.BytesIO(tile_raw_media))

                    if tile.mode.endswith(IMAGE_OPACITY_MODE):
                        tile_color_mode = tile.mode[:-1]
                        full_image_opacity_mode = IMAGE_OPACITY_MODE
                    else:
                        tile_color_mode = tile.mode

                    if tile_color_mode not in IMAGE_COLOR_MODES:
                        log.error("Resource %s tile %s has unexpected image mode %s" % (resource_name, tile_location, tile.mode))
                    elif IMAGE_COLOR_MODES.index(tile_color_mode) > IMAGE_COLOR_MODES.index(full_image_color_mode):
                        full_image_color_mode = tile_color_mode
                else:
                    tile = None
                    missing_tiles.append((x, y))

                tile_images.append(tile)
                tile_num += 1

        if missing_tiles:
            log.error("Resource %s is missing tiles: %s" % (resource_name, repr(missing_tiles)))
            if ignore_variants:
                return None, None

        full_image = Image.new(full_image_color_mode + full_image_opacity_mode, (resource_width, resource_height))

        for y, row in enumerate(yj_tiles):
            top_padding = 0 if y == 0 else tile_padding
            bottom_padding = min(tile_padding, resource_height - tile_height * (y + 1))

            for x, tile_location in enumerate(row):
                left_padding = 0 if x == 0 else tile_padding
                right_padding = min(tile_padding, resource_width - tile_width * (x + 1))

                tile = tile_images.pop(0)
                if tile is not None:
                    twidth, theight = tile.size
                    if twidth != tile_width + left_padding + right_padding or theight != tile_height + top_padding + bottom_padding:
                        log.error("Resource %s tile %d, %d size (%d, %d) does not have padding %d of expected size (%d, %d)" % (
                            resource_name, x, y, twidth, theight, tile_padding, tile_width, tile_height))
                        log.info("tile padding ltrb: %d, %d, %d, %d" % (left_padding, top_padding, right_padding, bottom_padding))

                    crop = (left_padding, top_padding, tile_width + left_padding, tile_height + top_padding)
                    tile = tile.crop(crop)
                    full_image.paste(tile, (x * tile_width, y * tile_height))
                    tile.close()

        if full_image.size != (resource_width, resource_height):
            log.error("Resource %s combined tiled image size is (%d, %d) but should be (%d, %d)" % (
                    resource_name, full_image.size[0], full_image.size[1], resource_width, resource_height))

        if resource_format == "$285" and COMBINE_TILES_LOSSLESS:
            resource_format = "$284"

        fmt = SYMBOL_FORMATS[resource_format]

        if fmt == "jpg":
            desired_combined_size = max(int(separate_tiles_size * COMBINED_TILE_SIZE_FACTOR), 1024)
            raw_media, quality = optimize_jpeg_image_quality(full_image, desired_combined_size)

            if DEBUG_TILES:
                size_diff = len(raw_media) - desired_combined_size
                diff_percentage = (size_diff * 100) // desired_combined_size
                if abs(diff_percentage) >= TILE_SIZE_REPORT_PERCENTAGE:
                    log.warning("Image resource %s has %d tiles %d bytes combined into quality %d %s JPEG %d bytes (%+d%%)" % (
                        resource_name, tile_count, separate_tiles_size, quality, full_image.mode, len(raw_media), diff_percentage))
        else:
            outfile = io.BytesIO()
            full_image.save(outfile, fmt)
            raw_media = outfile.getvalue()
            outfile.close()

        full_image.close()

    return raw_media, resource_format


def optimize_jpeg_image_quality(jpeg_image, desired_size):
    min_quality = MIN_JPEG_QUALITY
    max_quality = MAX_JPEG_QUALITY
    best_size_diff = best_quality = best_raw_media = None

    while max_quality >= min_quality:
        quality = (max_quality + min_quality) // 2
        outfile = io.BytesIO()
        jpeg_image.save(outfile, "JPEG", quality=quality, optimize=True)
        raw_media = outfile.getvalue()
        outfile.close()

        size_diff = len(raw_media) - desired_size

        if best_size_diff is None or abs(size_diff) < abs(best_size_diff):
            best_size_diff = size_diff
            best_quality = quality
            best_raw_media = raw_media

        if len(raw_media) < desired_size:
            min_quality = quality + 1
        else:
            max_quality = quality - 1

    return best_raw_media, best_quality


def get_pdf_page_size(pdf_data, resource_name, page_num):
    raw_media_file = io.BytesIO(pdf_data)
    pdf = pypdf.PdfReader(raw_media_file)
    page = pdf.pages[page_num - 1]

    if page.user_unit != 1:
        log.warning("PDF resource %s page %d, user_unit %f -- dimensions are not in points" % (
            resource_name, page_num, page.user_unit))

    crop_width, crop_height = box_size(page.cropbox)
    media_width, media_height = box_size(page.mediabox)

    return (crop_width, crop_height) if crop_width * crop_height <= media_width * media_height else (media_width, media_height)


def show_pdf_page_boxes(pdf_data, resource_name, page_num):
    raw_media_file = io.BytesIO(pdf_data)
    pdf = pypdf.PdfReader(raw_media_file)
    page = pdf.pages[page_num - 1]

    def box_repr(box):
        return "%s size %s" % (repr(box_tuple(box)), repr(box_size(box)))

    log.info("PDF resource %s page %d, user_unit %f" % (resource_name, page_num, page.user_unit))
    log.info("  artbox %s" % box_repr(page.artbox))
    log.info("  bleedbox %s" % box_repr(page.bleedbox))
    log.info("  cropbox %s" % box_repr(page.cropbox))
    log.info("  mediabox %s" % box_repr(page.mediabox))
    log.info("  trimbox %s" % box_repr(page.trimbox))


def box_tuple(box):
    return (box.lower_left[0], box.lower_left[1], box.upper_right[0], box.upper_right[1])


def box_size(box):
    return (box.upper_right[0] - box.lower_left[0], box.upper_right[1] - box.lower_left[1])


def crop_image(raw_media, resource_name, resource_width, resource_height, margin_left, margin_right, margin_top, margin_bottom):

    with disable_debug_log():
        img = Image.open(io.BytesIO(raw_media))
        orig_width, orig_height = img.size
        crop_left = int(margin_left * orig_width / resource_width)
        crop_right = orig_width - int(margin_right * orig_width / resource_width) - 1
        crop_top = int(margin_top * orig_height / resource_height)
        crop_bottom = orig_height - int(margin_bottom * orig_height / resource_height) - 1

        if crop_right < crop_left or crop_bottom < crop_top:
            log.warning("cropping entire %s image resource %s (%d, %d) by (%d, %d, %d, %d)" % (
                img.format, resource_name, orig_width, orig_height, crop_left, crop_top, crop_right, crop_bottom))

        cropped_img = img.crop((crop_left, crop_top, crop_right, crop_bottom))

        if img.format == "JPEG":
            cropped_raw_media = optimize_jpeg_image_quality(cropped_img, len(raw_media) * 0.6)[0]
        else:
            cropped_file = io.BytesIO()
            cropped_img.save(cropped_file, img.format)
            cropped_img.close()
            cropped_raw_media = cropped_file.getvalue()
            cropped_file.close()

        img.close()

    return cropped_raw_media


def jpeg_type(data, fmt="jpg"):

    if fmt not in ["jpg", "jpeg"]:
        return fmt.upper()

    if not data.startswith(b"\xff\xd8"):
        return "UNKNOWN(%s)" % data[:12].hex()

    if data[2:4] == b"\xff\xe0" and data[6:10] == b"JFIF":
        return "JPEG"

    if data[2:4] == b"\xff\xe1" and data[6:10] == b"Exif":
        return "JPEG/Exif"

    if data[2:4] == b"\xff\xe8":
        return "JPEG/SPIFF"

    if data[2:4] in [b"\xff\xed", b"\xff\xee"]:
        return "JPEG/Adobe"

    if data[2:4] in [b"\xff\xdb", b"\xff\xde"]:
        return "JPEG/no-app-marker"

    return "JPEG/UNKNOWN(%s)" % data[:12].hex()


def font_file_ext(data, default=""):
    if data[0:4] in {b"\x00\x01\x00\x00", b"true", b"typ1"}:
        return ".ttf"

    if data[0:4] == b"OTTO":
        return ".otf"

    if data[0:4] == b"wOFF":
        return ".woff"

    if data[0:4] == b"wOF2":
        return ".woff2"

    if data[34:36] == b"\x4c\x50" and data[8:12] in {b"\x00\x00\x01\x00", b"\x01\x00\x02\x00", b"\x02\x00\x02\x00"}:
        return ".eot"

    if data[0:4] == b"\x00\x00\x01\x00":
        return ".dfont"

    if data[0:2] == b"\x80\x01" and data[6:24] == b"%!PS-AdobeFont-1.0":
        return ".pfb"

    return default


def image_file_ext(data, default=""):
    if data.startswith(b"GIF87a") or data.startswith(b"GIF89a"):
        return ".gif"

    if data.startswith(b"\xff\xd8\xff"):
        return ".jpg"

    if data.startswith(b"\x49\x49\xbc\x01"):
        return ".jxr"

    if data.startswith(b"\x89PNG\x0d\x0a\x1a\x0a"):
        return ".png"

    if data.startswith(b"%PDF"):
        return ".pdf"

    if data.startswith(b"\x49\x49\x2a\x00") or data.startswith(b"\x4d\x4d\x00\x2a"):
        return ".tif"

    return default


def image_size(data):
    with disable_debug_log():
        cover_im = Image.open(io.BytesIO(data))
        width, height = cover_im.size
        cover_im.close()

    return (width, height)
