loc:
	wc -l mm-device/*/*rs \
		mm-core/*/*rs \
		adapters/*/*/*rs

demo:
	cargo run -p mm-demo
