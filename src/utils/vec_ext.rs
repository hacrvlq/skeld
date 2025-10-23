pub trait VecExt<T, K> {
	fn get_maximums_by_key(self, key_fn: impl Fn(&T) -> K) -> Vec<T>;
}

impl<T, K: Ord> VecExt<T, K> for Vec<T> {
	fn get_maximums_by_key(self, key_fn: impl Fn(&T) -> K) -> Vec<T> {
		let mut max_elements: Vec<T> = Vec::new();
		for element in self {
			if max_elements
				.first()
				.is_some_and(|current_max| key_fn(current_max) > key_fn(&element))
			{
				continue;
			}

			if max_elements
				.first()
				.is_some_and(|current_max| key_fn(current_max) < key_fn(&element))
			{
				max_elements.clear();
			}

			max_elements.push(element);
		}
		max_elements
	}
}
