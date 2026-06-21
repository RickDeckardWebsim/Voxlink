import urllib.request
import json

URL = "https://syftqwloslmnjyvppler.supabase.co/auth/v1/signup"
ANON_KEY = "sb_publishable_VK3kO0lX4tTsrHlCsH6JFQ_ebB6_lMH"

req = urllib.request.Request(URL, method="POST")
req.add_header("apikey", ANON_KEY)
req.add_header("Content-Type", "application/json")
data = json.dumps({"email": "test4829348@example.com", "password": "password123"}).encode("utf-8")

try:
    with urllib.request.urlopen(req, data=data) as response:
        print(response.read().decode())
except urllib.error.HTTPError as e:
    print(f"HTTP Error {e.code}: {e.read().decode()}")
except Exception as e:
    print(e)
