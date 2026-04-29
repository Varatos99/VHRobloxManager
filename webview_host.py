#!/usr/bin/env python3
"""
webview_host.py - Chrome CDP ile .ROBLOSECURITY cookie'sini yakalar.
Kullanici sadece giris yapar, cookie otomatik yakalanir.
"""
import json
import time
import subprocess
import sys
import os
import urllib.request
import websocket
import tempfile
import logging

logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(message)s')
logger = logging.getLogger(__name__)

def find_browser():
    paths = [
        "C:/Program Files/Google/Chrome/Application/chrome.exe",
        "C:/Program Files (x86)/Google/Chrome/Application/chrome.exe",
        "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
        "C:/Program Files/Microsoft/Edge/Application/msedge.exe",
    ]
    for path in paths:
        if os.path.exists(path):
            return path
    try:
        result = subprocess.run(["where", "chrome"], capture_output=True, text=True)
        if result.returncode == 0:
            return result.stdout.strip().split('\n')[0]
    except:
        pass
    return None

def get_page_websocket_url():
    """Get the WebSocket URL for the Roblox page."""
    try:
        req = urllib.request.urlopen("http://localhost:9222/json/list", timeout=2)
        pages = json.loads(req.read())
        for page in pages:
            url = page.get("url", "")
            if "roblox.com" in url:
                ws_url = page.get("webSocketDebuggerUrl")
                if ws_url:
                    logger.info(f"Found Roblox page: {url}")
                    return ws_url
    except Exception as e:
        logger.info(f"Error getting page list: {e}")
    return None

def main():
    browser = find_browser()
    if not browser:
        print("No Chrome/Edge found", file=sys.stderr)
        sys.exit(1)

    # Create unique temp directory for clean profile
    temp_profile = tempfile.mkdtemp(prefix="rm_chrome_")
    logger.info(f"Using fresh profile: {temp_profile}")

    # Start browser with remote debugging
    chrome = subprocess.Popen([
        browser,
        "--remote-debugging-port=9222",
        "--remote-allow-origins=*",
        f"--user-data-dir={temp_profile}",
        "--no-first-run",
        "--no-default-browser-check",
        "--new-window",
        "https://www.roblox.com/login"
    ], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)

    # Wait for browser and page to load
    logger.info("Waiting for browser...")
    time.sleep(3)

    # Get the page's WebSocket URL
    page_ws_url = None
    start = time.time()
    while time.time() - start < 15:
        page_ws_url = get_page_websocket_url()
        if page_ws_url:
            break
        time.sleep(0.5)

    if not page_ws_url:
        print("No Roblox page found", file=sys.stderr)
        chrome.kill()
        sys.exit(1)

    # Connect to the PAGE's WebSocket (not browser's)
    logger.info(f"Connecting to page WebSocket...")
    ws = websocket.create_connection(page_ws_url, timeout=30)
    logger.info("Connected to page!")

    # Enable Network
    ws.send(json.dumps({"id": 1, "method": "Network.enable"}))
    time.sleep(0.5)

    # Poll for cookie
    msg_id = 2
    start = time.time()
    cookie_found = False

    while time.time() - start < 120:
        # Check if browser still running
        if chrome.poll() is not None:
            print("Browser exited", file=sys.stderr)
            break

        # Check if page still exists
        if not get_page_websocket_url():
            print("Page closed", file=sys.stderr)
            break

        # Send getCookies request
        try:
            ws.send(json.dumps({
                "id": msg_id,
                "method": "Network.getCookies"
            }))
            logger.info(f"Sent getCookies (id={msg_id})")
        except Exception as e:
            logger.info(f"Send error: {e}")
            break

        # Wait for response
        response_wait_start = time.time()
        while time.time() - response_wait_start < 2:
            try:
                ws.settimeout(0.5)
                msg = ws.recv()
                data = json.loads(msg)

                # Check if this is the response to our request
                if "id" in data and data["id"] == msg_id:
                    logger.info("Got response, checking cookies...")
                    if "result" in data and "cookies" in data.get("result", {}):
                        cookies = data["result"]["cookies"]
                        logger.info(f"Found {len(cookies)} cookies")
                        for cookie in cookies:
                            name = cookie.get("name", "")
                            domain = cookie.get("domain", "")
                            logger.info(f"  {name} (domain: {domain})")
                            if name == ".ROBLOSECURITY":
                                cookie_value = cookie["value"]
                                logger.info("Cookie found!")
                                print(cookie_value)
                                sys.stdout.flush()
                                cookie_found = True
                                break
                    break  # Exit response wait loop

                # Check for navigation events
                elif data.get("method") == "Page.frameNavigated":
                    try:
                        url = data["params"]["frame"]["url"]
                        logger.info(f"Navigation: {url}")
                    except:
                        pass

            except websocket.WebSocketTimeoutException:
                break  # Exit response wait, send new request
            except Exception as e:
                logger.info(f"Receive error: {e}")
                break

        if cookie_found:
            break

        msg_id += 1
        time.sleep(1)  # Wait before next poll

    # Cleanup
    if not cookie_found:
        print("Timeout - cookie not found", file=sys.stderr)

    try:
        chrome.kill()
    except:
        pass

    # Clean up temp directory
    try:
        import shutil
        shutil.rmtree(temp_profile, ignore_errors=True)
    except:
        pass

    sys.exit(0 if cookie_found else 1)

if __name__ == "__main__":
    main()
