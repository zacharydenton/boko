import init, { epub_to_azw3, epub_to_kfx, mobi_to_epub, mobi_to_azw3, mobi_to_kfx, kfx_to_epub, kfx_to_azw3 } from './pkg/boko.js';

// DOM elements
const dropzone = document.getElementById('dropzone');
const fileInput = document.getElementById('file-input');
const fileInfo = document.getElementById('file-info');
const fileName = document.getElementById('file-name');
const fileSize = document.getElementById('file-size');
const clearFileBtn = document.getElementById('clear-file');
const conversionOptions = document.getElementById('conversion-options');
const outputFormat = document.getElementById('output-format');
const convertBtn = document.getElementById('convert-btn');
const progress = document.getElementById('progress');
const progressFill = document.getElementById('progress-fill');
const progressText = document.getElementById('progress-text');
const result = document.getElementById('result');
const downloadLink = document.getElementById('download-link');
const error = document.getElementById('error');
const errorMessage = document.getElementById('error-message');

let wasmReady = false;
let currentFile = null;

// Initialize WASM
async function initWasm() {
    try {
        await init();
        wasmReady = true;
    } catch (e) {
        showError('Failed to load WASM module: ' + e.message);
    }
}

// File handling
function getFileExtension(filename) {
    return filename.split('.').pop().toLowerCase();
}

function formatFileSize(bytes) {
    if (bytes < 1024) return bytes + ' B';
    if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + ' KB';
    return (bytes / (1024 * 1024)).toFixed(1) + ' MB';
}

function getInputFormat(filename) {
    const ext = getFileExtension(filename);
    if (ext === 'epub') return 'epub';
    if (ext === 'azw3' || ext === 'mobi') return 'mobi';
    if (ext === 'kfx') return 'kfx';
    return null;
}

function updateOutputOptions(inputFormat) {
    outputFormat.innerHTML = '';

    if (inputFormat === 'epub') {
        outputFormat.innerHTML = `
            <option value="azw3">AZW3</option>
            <option value="kfx">KFX</option>
        `;
    } else if (inputFormat === 'mobi') {
        outputFormat.innerHTML = `
            <option value="epub">EPUB</option>
            <option value="azw3">AZW3</option>
            <option value="kfx">KFX</option>
        `;
    } else if (inputFormat === 'kfx') {
        outputFormat.innerHTML = `
            <option value="epub">EPUB</option>
            <option value="azw3">AZW3</option>
        `;
    }
}

function handleFile(file) {
    const inputFormat = getInputFormat(file.name);

    if (!inputFormat) {
        showError('Unsupported file format. Please use EPUB, AZW3, MOBI, or KFX.');
        return;
    }

    currentFile = file;

    // Update UI
    fileName.textContent = file.name;
    fileSize.textContent = formatFileSize(file.size);
    fileInfo.classList.remove('hidden');
    dropzone.classList.add('hidden');

    updateOutputOptions(inputFormat);
    conversionOptions.classList.remove('hidden');

    // Hide previous results/errors
    result.classList.add('hidden');
    error.classList.add('hidden');
}

function clearFile() {
    currentFile = null;
    fileInput.value = '';

    fileInfo.classList.add('hidden');
    conversionOptions.classList.add('hidden');
    result.classList.add('hidden');
    error.classList.add('hidden');
    dropzone.classList.remove('hidden');
}

function showError(message) {
    errorMessage.textContent = message;
    error.classList.remove('hidden');
    progress.classList.add('hidden');
    result.classList.add('hidden');
}

function showProgress(message) {
    progressText.textContent = message;
    progressFill.style.width = '50%';
    progress.classList.remove('hidden');
    result.classList.add('hidden');
    error.classList.add('hidden');
}

function showResult(blob, filename) {
    const url = URL.createObjectURL(blob);
    downloadLink.href = url;
    downloadLink.download = filename;

    progress.classList.add('hidden');
    result.classList.remove('hidden');
}

async function convert() {
    if (!wasmReady) {
        showError('WASM module not ready. Please refresh the page.');
        return;
    }

    if (!currentFile) {
        showError('No file selected.');
        return;
    }

    const inputFormat = getInputFormat(currentFile.name);
    const targetFormat = outputFormat.value;

    showProgress('Reading file...');

    try {
        const arrayBuffer = await currentFile.arrayBuffer();
        const inputData = new Uint8Array(arrayBuffer);

        showProgress('Converting...');

        let outputData;
        let outputFilename;
        let mimeType;

        const baseName = currentFile.name.replace(/\.[^/.]+$/, '');

        if (inputFormat === 'epub' && targetFormat === 'azw3') {
            outputData = epub_to_azw3(inputData);
            outputFilename = baseName + '.azw3';
            mimeType = 'application/x-mobi8-ebook';
        } else if (inputFormat === 'epub' && targetFormat === 'kfx') {
            outputData = epub_to_kfx(inputData);
            outputFilename = baseName + '.kfx';
            mimeType = 'application/octet-stream';
        } else if (inputFormat === 'mobi' && targetFormat === 'epub') {
            outputData = mobi_to_epub(inputData);
            outputFilename = baseName + '.epub';
            mimeType = 'application/epub+zip';
        } else if (inputFormat === 'mobi' && targetFormat === 'azw3') {
            outputData = mobi_to_azw3(inputData);
            outputFilename = baseName + '.azw3';
            mimeType = 'application/x-mobi8-ebook';
        } else if (inputFormat === 'mobi' && targetFormat === 'kfx') {
            outputData = mobi_to_kfx(inputData);
            outputFilename = baseName + '.kfx';
            mimeType = 'application/octet-stream';
        } else if (inputFormat === 'kfx' && targetFormat === 'epub') {
            outputData = kfx_to_epub(inputData);
            outputFilename = baseName + '.epub';
            mimeType = 'application/epub+zip';
        } else if (inputFormat === 'kfx' && targetFormat === 'azw3') {
            outputData = kfx_to_azw3(inputData);
            outputFilename = baseName + '.azw3';
            mimeType = 'application/x-mobi8-ebook';
        } else {
            throw new Error('Unsupported conversion: ' + inputFormat + ' to ' + targetFormat);
        }

        const blob = new Blob([outputData], { type: mimeType });
        showResult(blob, outputFilename);

    } catch (e) {
        showError('Conversion failed: ' + e.message);
    }
}

// Event listeners
dropzone.addEventListener('click', () => fileInput.click());

dropzone.addEventListener('dragover', (e) => {
    e.preventDefault();
    dropzone.classList.add('dragover');
});

dropzone.addEventListener('dragleave', () => {
    dropzone.classList.remove('dragover');
});

dropzone.addEventListener('drop', (e) => {
    e.preventDefault();
    dropzone.classList.remove('dragover');

    const files = e.dataTransfer.files;
    if (files.length > 0) {
        handleFile(files[0]);
    }
});

fileInput.addEventListener('change', (e) => {
    if (e.target.files.length > 0) {
        handleFile(e.target.files[0]);
    }
});

clearFileBtn.addEventListener('click', clearFile);
convertBtn.addEventListener('click', convert);

// Initialize
initWasm();
