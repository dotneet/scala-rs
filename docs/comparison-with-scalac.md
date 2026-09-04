## Comparison with scalac 2.13

The honest diff.

- **Scale**: a very small part of nsc. It does not meet the language
  specification.
- **Library**: by default, **`compile` / `run`** link against the jar when one
  can be auto-detected and emit no private class file of the same name; when
  none is found they fall back to the private runtime. `--scala-library` (with
  the path omitted, it searches `SCALA_LIBRARY_JAR` / `/tmp/scala-rs-lib` / the
  cwd) states it explicitly. **`--no-scala-library` forces the private
  runtime.** What rides on the jar: `Option` / `Some` / `None` / `List` / `Nil` /
  `::` / `Function0` / `Function1` / `Tuple2` / `NotImplementedError` /
  `Predef$` (`println` / `assert` / `require` / `???` / `identity` / `locally` /
  `implicitly`) / `any2stringadd` / `->` from `ArrowAssoc` / `intWrapper` /
  `RichInt` (`abs` / `max` / `min` / `to` / `until`) / `longWrapper` /
  `RichLong` (`abs` / `max` / `min` / `to` / `until` → a real
  `NumericRange[Long]`) / `doubleWrapper` / `RichDouble` (`abs` / `max` /
  `min`) / `floatWrapper` / `RichFloat` (`abs` / `max` / `min`) / `charWrapper` /
  `RichChar` (`isDigit` / `toInt` via `intValue$extension` / `to` / `until` → a
  real `NumericRange[Char]`) / `byteWrapper` / `RichByte` (`abs` / `max` / `min` /
  `to` / `until` → a real `NumericRange[Byte]`) / `shortWrapper` / `RichShort`
  (`abs` / `max` / `min` / `to` / `until` → a real `NumericRange[Short]`) /
  `booleanWrapper` / `RichBoolean.compare` (the instance `compare(Object)`) /
  `StringOps` (`toInt$extension` / `size$extension` / `$times$extension` /
  `take$extension` / `drop$extension` / `isEmpty` via `augmentString` /
  `toUpperCase`/`toLowerCase` inlined to `String` / `stripPrefix$extension` /
  `split$extension` / `stripSuffix$extension` / `padTo$extension(Int,Char)` /
  `linesIterator$extension` / `toIntOption$extension` / `stripMargin$extension` /
  `lines$extension` / `capitalize$extension` / `reverse$extension` /
  `slice$extension` / `takeRight$extension` / `dropRight$extension` /
  `contains$extension(Char)` / `head$extension` / `last$extension` /
  `stripLineEnd$extension` / `replaceAllLiterally$extension` / `tail$extension` /
  `init$extension` / `distinct$extension` / `mkString$extension`) / `WithFilter` /
  `Iterator` / `Map` / `Vector` / `IndexedSeq` (unqualified
  `IndexedSeq(1, 2)(1)`) / `Queue` (`enqueue` / `dequeue` of
  `scala.collection.immutable.Queue`) / `ArrayBuffer` (varargs `apply` / `+=` /
  `apply` / `update` of `scala.collection.mutable.ArrayBuffer`) / `ListBuffer`
  (varargs `apply` / `+=` / `apply` of
  `scala.collection.mutable.ListBuffer`) / `StringBuilder` (`new` / `+=` /
  `append` / `toString` of `scala.collection.mutable.StringBuilder`) / `HashMap`
  (companion `empty` / varargs `apply` / `update` / `+=` / `apply` / `get` of
  `scala.collection.mutable.HashMap`) / `HashSet` (companion `empty` / varargs
  `apply` / `+=` / `contains` of `scala.collection.mutable.HashSet`) /
  `LinkedHashMap` (companion `empty` / varargs `apply` / `update` / `+=` /
  `apply` / insertion-order `foreach` of
  `scala.collection.mutable.LinkedHashMap`; `HashMap` guarantees no order) /
  `LinkedHashSet` (companion `empty` / varargs `apply` / `+=` / `contains` /
  insertion-order `foreach` of `scala.collection.mutable.LinkedHashSet`) /
  `ArrayDeque` (companion `empty` / varargs `apply` / `+=` / `prepend` / `apply`
  of `scala.collection.mutable.ArrayDeque`) / `ArrayOps` (`head` / `tail` /
  `foreach` / `map[B: ClassTag]` via `intArrayOps`; `head` / `foreach` via
  `longArrayOps`; `map` on reference arrays via `refArrayOps`; no private
  `ArrayOps` class file is emitted) / `Set` / `Seq` / `LazyList` (`empty` /
  `foreach` / **varargs `apply`**) / `Either` (`Left` / `Right` and right-biased
  `isLeft` / `isRight` / `map` / `flatMap` / `fold` / `getOrElse` / `orElse` /
  `swap` / `toOption` / `toSeq` / `contains` / `exists` / `forall` / `foreach` /
  `filterOrElse` / `left`; no `Either$LeftProjection` class file is emitted) /
  `Try` (`apply` on `Try$` / `Success` / `Failure`, plus `isSuccess` /
  `isFailure` / `get` / `getOrElse` / `map` / `flatMap` / `filter` / `withFilter`
  (`Try$WithFilter`) / `foreach` / `orElse` / `recover` / `recoverWith` /
  `collect` / `toOption` / `toEither` / `failed` / `transform` / `fold`) /
  `Array$` (varargs `apply` + `ClassTag`). Dual-run: `hello` / `option_for` /
  `list_for` / `predef` / `predef_more` / `unapply` / `unapply_seq` / `iterator` /
  `map` / `vector` / `int_ops` / `string_ops` / `list_apply` / `set` / `long_ops` /
  `seq` / `either` / `float_ops` / `string_ops2` / `anonymous` / `eta` /
  `try_util` / `existentials` / `existential_bounds` / `implicit_specific` /
  `lambda_lift` / `view_bounds` / `view_bounds_class` / `hk_types` / `app` /
  `delayed_init` / `implicit_inherit_local` / `partial_function` /
  `list_collect` / `string_interp` / `overloading` / `classtag` /
  `context_bounds` / `context_bounds_class` / `type_member_hk` / `refine_hk` /
  `refine_bound` / `nested_proj` / `type_member_bounds` / `assign_op` /
  `collection_converters` / `pkg_implicit_class` / `structural_update` /
  `indexedseq_queue` / `string_ops3` / `byte_ops` / `arraybuffer` / `string_ops4` /
  `numeric_range` / `listbuffer` / `string_ops5` / `short_range` /
  `stringbuilder` / `string_ops6` / `long_range` / `hashmap` / `string_ops7` /
  `char_range` / `hashset` / `string_ops8` / `array_ops2` / `linkedhashmap` /
  `string_ops9` / `array_ops3` / `linkedhashset` / `string_ops10` / `array_ops4` /
  `arraydeque` / `custom_interp` / `array_ops` / `either_ops` / `either_left` /
  `either_for` / `option_x1` / `option_x2` / `try_ops` / `try_recover` /
  `try_for`. **Still intrinsic / private, or not linked**: the rest of
  `StringOps`, the rest of the numerics, the other mutable collections.
  `List.unapplySeq` is `SeqOps`'s identity in the library. The varargs `apply` of
  `List`/`Seq`/`LazyList`/`Array` is **library only**.
- **Library**: by default, **`compile` / `run`** link against the jar when one
  can be auto-detected and emit no private class file of the same name; when
  none is found they fall back to the private runtime. `--scala-library` (with
  the path omitted, it searches `SCALA_LIBRARY_JAR` / `/tmp/scala-rs-lib` / the
  cwd) states it explicitly. **`--no-scala-library` forces the private
  runtime.** What rides on the jar: `Option` / `Some` / `None` / `List` / `Nil` /
  `::` / `Function0` / `Function1` / `Tuple2` / `NotImplementedError` /
  `Predef$` (`println` / `assert` / `require` / `???` / `identity` / `locally` /
  `implicitly`) / `any2stringadd` / `->` from `ArrowAssoc` / `intWrapper` /
  `RichInt` (`abs` / `max` / `min` / `to` / `until` / `toBinaryString` /
  `toHexString` / `toOctalString` / `sign`; `Range` (`withFilter` / `filter` /
  `map` / `flatMap` / `foldLeft` / `foldRight` / `sum` / `product` / `min` /
  `max` / `toList` / `toVector` / `zipWithIndex` / `take` / `drop`, …) and
  `scala.math` (`abs` / `max` / `min` / `signum` / `pow` / `sqrt` / `floor` /
  `ceil` / `round` / `random`) came along too) / `longWrapper` / `RichLong`
  (`abs` / `max` / `min` / `to` / `until` → a real `NumericRange[Long]`) /
  `doubleWrapper` / `RichDouble` (`abs` / `max` / `min`) / `floatWrapper` /
  `RichFloat` (`abs` / `max` / `min`) / `charWrapper` / `RichChar` (`isDigit` /
  `toInt` via `intValue$extension` / `to` / `until` → a real
  `NumericRange[Char]`) / `byteWrapper` / `RichByte` (`abs` / `max` / `min` /
  `to` / `until` → a real `NumericRange[Byte]`) / `shortWrapper` / `RichShort`
  (`abs` / `max` / `min` / `to` / `until` → a real `NumericRange[Short]`) /
  `booleanWrapper` / `RichBoolean.compare` (the instance `compare(Object)`) /
  `StringOps` (`toInt$extension` / `size$extension` / `$times$extension` /
  `take$extension` / `drop$extension` / `isEmpty` via `augmentString` /
  `toUpperCase`/`toLowerCase` inlined to `String` / `stripPrefix$extension` /
  `split$extension` / `stripSuffix$extension` / `padTo$extension(Int,Char)` /
  `linesIterator$extension` / `toIntOption$extension` / `stripMargin$extension` /
  `lines$extension` / `capitalize$extension` / `reverse$extension` /
  `slice$extension` / `takeRight$extension` / `dropRight$extension` /
  `contains$extension(Char)` / `head$extension` / `last$extension` /
  `stripLineEnd$extension` / `replaceAllLiterally$extension` / `tail$extension` /
  `init$extension` / `distinct$extension` / `mkString$extension`) / `WithFilter` /
  `Iterator` / `Map` / `Vector` / `IndexedSeq` (unqualified
  `IndexedSeq(1, 2)(1)`) / `Queue` (`enqueue` / `dequeue` of
  `scala.collection.immutable.Queue`) / `ArrayBuffer` (varargs `apply` / `+=` /
  `apply` / `update` of `scala.collection.mutable.ArrayBuffer`) / `ListBuffer`
  (varargs `apply` / `+=` / `apply` of `scala.collection.mutable.ListBuffer`) /
  `StringBuilder` (the bare name / `new` / every `append` overload / `+=` /
  `++=` / `insert` / `deleteCharAt` / `setLength` / `reverse` / `clear` /
  `toString` / `result` of `scala.collection.mutable.StringBuilder`) / `HashMap`
  (companion `empty` / varargs `apply` / `update` / `+=` / `apply` / `get` of
  `scala.collection.mutable.HashMap`) / `HashSet` (companion `empty` / varargs
  `apply` / `+=` / `contains` of `scala.collection.mutable.HashSet`) /
  `LinkedHashMap` (companion `empty` / varargs `apply` / `update` / `+=` /
  `apply` / insertion-order `foreach` of
  `scala.collection.mutable.LinkedHashMap`; `HashMap` guarantees no order) /
  `LinkedHashSet` (companion `empty` / varargs `apply` / `+=` / `contains` /
  insertion-order `foreach` of `scala.collection.mutable.LinkedHashSet`) /
  `ArrayDeque` (companion `empty` / varargs `apply` / `+=` / `prepend` / `apply`
  of `scala.collection.mutable.ArrayDeque`) / `ArrayOps` (`head` / `tail` /
  `foreach` / `map[B: ClassTag]` via `intArrayOps`; `head` / `foreach` via
  `longArrayOps`; `map` on reference arrays via `refArrayOps`; no private
  `ArrayOps` class file is emitted) / `Set` / `Seq` / `LazyList` (`empty` /
  `foreach` / **varargs `apply`**) / `Either` (`Left` / `Right` / `isLeft` /
  `getOrElse` / `map`) / `Try` (`apply` / `map` / `getOrElse` on `Try$` /
  `Success` / `Failure`) / `Array$` (varargs `apply` + `ClassTag`). Dual-run:
  `hello` / `option_for` / `list_for` / `predef` / `predef_more` / `unapply` /
  `unapply_seq` / `iterator` / `map` / `vector` / `int_ops` / `string_ops` /
  `list_apply` / `set` / `long_ops` / `seq` / `either` / `float_ops` /
  `string_ops2` / `anonymous` / `eta` / `try_util` / `existentials` /
  `existential_bounds` / `implicit_specific` / `lambda_lift` / `view_bounds` /
  `view_bounds_class` / `hk_types` / `app` / `delayed_init` /
  `implicit_inherit_local` / `partial_function` / `list_collect` /
  `string_interp` / `overloading` / `classtag` / `context_bounds` /
  `context_bounds_class` / `type_member_hk` / `refine_hk` / `refine_bound` /
  `nested_proj` / `type_member_bounds` / `assign_op` / `collection_converters` /
  `pkg_implicit_class` / `structural_update` / `indexedseq_queue` /
  `string_ops3` / `byte_ops` / `arraybuffer` / `string_ops4` / `numeric_range` /
  `listbuffer` / `string_ops5` / `short_range` / `stringbuilder` / `string_ops6` /
  `long_range` / `hashmap` / `string_ops7` / `char_range` / `hashset` /
  `string_ops8` / `array_ops2` / `linkedhashmap` / `string_ops9` / `array_ops3` /
  `linkedhashset` / `string_ops10` / `array_ops4` / `arraydeque` /
  `custom_interp` / `array_ops`. **Still intrinsic / private, or not linked**:
  the rest of `StringOps`, the rest of the numerics, the other mutable
  collections. `List.unapplySeq` is `SeqOps`'s identity in the library. The
  varargs `apply` of `List`/`Seq`/`LazyList`/`Array` is **library only**.
- **Library**: by default, **`compile` / `run`** link against the jar when one
  can be auto-detected and emit no private class file of the same name; when
  none is found they fall back to the private runtime. `--scala-library` (with
  the path omitted, it searches `SCALA_LIBRARY_JAR` / `/tmp/scala-rs-lib` / the
  cwd) states it explicitly. **`--no-scala-library` forces the private
  runtime.** What rides on the jar: `Option` / `Some` / `None` / `List` / `Nil` /
  `::` / `Function0` / `Function1` / `Tuple2` (`_1` / `_2`, plus `swap` /
  `toString`) / `NotImplementedError` / `Predef$` (`println` / `assert` /
  `require` / `???` / `identity` / `locally` / `implicitly`) / `any2stringadd` /
  `->` from `ArrowAssoc` / `intWrapper` / `RichInt` (`abs` / `max` / `min` /
  `to` / `until`) / `longWrapper` / `RichLong` (`abs` / `max` / `min` / `to` /
  `until` → a real `NumericRange[Long]`) / `doubleWrapper` / `RichDouble` (`abs` /
  `max` / `min`) / `floatWrapper` / `RichFloat` (`abs` / `max` / `min`) /
  `charWrapper` / `RichChar` (`isDigit` / `toInt` via `intValue$extension` / `to` /
  `until` → a real `NumericRange[Char]`) / `byteWrapper` / `RichByte` (`abs` /
  `max` / `min` / `to` / `until` → a real `NumericRange[Byte]`) / `shortWrapper` /
  `RichShort` (`abs` / `max` / `min` / `to` / `until` → a real
  `NumericRange[Short]`) / `booleanWrapper` / `RichBoolean.compare` (the instance
  `compare(Object)`) / `StringOps` (`toInt$extension` / `size$extension` /
  `$times$extension` / `take$extension` / `drop$extension` / `isEmpty` via
  `augmentString` / `toUpperCase`/`toLowerCase` inlined to `String` /
  `stripPrefix$extension` / `split$extension` / `stripSuffix$extension` /
  `padTo$extension(Int,Char)` / `linesIterator$extension` /
  `toIntOption$extension` / `stripMargin$extension` / `lines$extension` /
  `capitalize$extension` / `reverse$extension` / `slice$extension` /
  `takeRight$extension` / `dropRight$extension` / `contains$extension(Char)` /
  `head$extension` / `last$extension` / `stripLineEnd$extension` /
  `replaceAllLiterally$extension` / `tail$extension` / `init$extension` /
  `distinct$extension` / `mkString$extension`) / `WithFilter` / `Iterator` /
  `Map` (`apply` / `get` / `updated` / `+` / `foreach`, plus `getOrElse` /
  `contains` / `keys` / `values` / `keySet` / `-` / `size` / `isEmpty` /
  `nonEmpty` / `filter` / `toList` / `toSeq` / `iterator` / `mkString` / `head` /
  `foldLeft` / `withDefaultValue` / `view` / `MapView.mapValues`) / `Vector`
  (`apply` / `length` / `updated` / `:+` / `foreach`, plus `size` / `isEmpty` /
  `nonEmpty` / `head` / `map` / `filter` / `toList` / `toSeq` / `iterator` /
  `mkString` / `foldLeft`) / `IndexedSeq` (unqualified `IndexedSeq(1, 2)(1)`) /
  `Queue` (`enqueue` / `dequeue` of `scala.collection.immutable.Queue`) /
  `ArrayBuffer` (varargs `apply` / `+=` / `apply` / `update` / `length` / `size` /
  `isEmpty` / `nonEmpty` / `head` / `last` / `mkString`(0/1/3) / `foreach` /
  `map` / `filter` / `toList` / `iterator` / `clear` / `remove` / `insert` /
  `contains` / `indexOf` / `reverse` / `foldLeft` / `append` / `++=` / `-=` /
  `sortBy` / `sorted` of `scala.collection.mutable.ArrayBuffer`) / `ListBuffer`
  (the same set of members of `scala.collection.mutable.ListBuffer`) / the new
  `mutable.Map[K, V]` and `mutable.Set[A]` (previously only `HashMap` / `HashSet`
  rode along; the `Map$` / `Set$` companions delegate at run time to `HashMap` /
  `HashSet` through `MapFactory$Delegate` / `IterableFactory$Delegate` while the
  static type stays the trait. `mutable.Map` has `apply` / `get` / `update` /
  `getOrElse` / `getOrElseUpdate` / `contains` / `keys` / `values` / `+=` / `-=` /
  `remove` / `size` / `isEmpty` / `nonEmpty` / `clear` / `foreach` / `filter` /
  `toList` / `toSeq` / `iterator` / `mkString`; `mutable.Set` has `contains` /
  `+=` / `-=` / `remove` / `size` / `isEmpty` / `nonEmpty` / `clear` / `foreach` /
  `map` / `filter` / `toList` / `toSeq` / `iterator` / `mkString`) /
  `StringBuilder` (`new` / `+=` / `append` / `toString` of
  `scala.collection.mutable.StringBuilder`) / `HashMap` (companion `empty` /
  varargs `apply` / `update` / `+=` / `apply` / `get` of
  `scala.collection.mutable.HashMap`) / `HashSet` (companion `empty` / varargs
  `apply` / `+=` / `contains` of `scala.collection.mutable.HashSet`) /
  `LinkedHashMap` (companion `empty` / varargs `apply` / `update` / `+=` /
  `apply` / insertion-order `foreach` of
  `scala.collection.mutable.LinkedHashMap`; `HashMap` guarantees no order) /
  `LinkedHashSet` (companion `empty` / varargs `apply` / `+=` / `contains` /
  insertion-order `foreach` of `scala.collection.mutable.LinkedHashSet`) /
  `ArrayDeque` (companion `empty` / varargs `apply` / `+=` / `prepend` / `apply`
  of `scala.collection.mutable.ArrayDeque`) / `ArrayOps` (`head` / `tail` /
  `foreach` / `map[B: ClassTag]` via `intArrayOps`; `head` / `foreach` via
  `longArrayOps`; `map` on reference arrays via `refArrayOps`; no private
  `ArrayOps` class file is emitted) / `Set` (`contains` / `foreach`, plus `+` /
  `-` / `++` / `size` / `isEmpty` / `nonEmpty` / `filter` / `map` / `toList` /
  `toSeq` / `iterator` / `mkString` / `head`) / `Seq` / `LazyList` (`empty` /
  `foreach` / **varargs `apply`**) / `Either` (`Left` / `Right` / `isLeft` /
  `getOrElse` / `map`) / `Try` (`apply` / `map` / `getOrElse` on `Try$` /
  `Success` / `Failure`) / `Array$` (varargs `apply` + `ClassTag`). Dual-run:
  `hello` / `option_for` / `list_for` / `predef` / `predef_more` / `unapply` /
  `unapply_seq` / `iterator` / `map` / `vector` / `int_ops` / `string_ops` /
  `list_apply` / `set` / `long_ops` / `seq` / `either` / `float_ops` /
  `string_ops2` / `anonymous` / `eta` / `try_util` / `existentials` /
  `existential_bounds` / `implicit_specific` / `lambda_lift` / `view_bounds` /
  `view_bounds_class` / `hk_types` / `app` / `delayed_init` /
  `implicit_inherit_local` / `partial_function` / `list_collect` /
  `string_interp` / `overloading` / `classtag` / `context_bounds` /
  `context_bounds_class` / `type_member_hk` / `refine_hk` / `refine_bound` /
  `nested_proj` / `type_member_bounds` / `assign_op` / `collection_converters` /
  `pkg_implicit_class` / `structural_update` / `indexedseq_queue` /
  `string_ops3` / `byte_ops` / `arraybuffer` / `string_ops4` / `numeric_range` /
  `listbuffer` / `string_ops5` / `short_range` / `stringbuilder` / `string_ops6` /
  `long_range` / `hashmap` / `string_ops7` / `char_range` / `hashset` /
  `string_ops8` / `array_ops2` / `linkedhashmap` / `string_ops9` / `array_ops3` /
  `linkedhashset` / `string_ops10` / `array_ops4` / `arraydeque` /
  `custom_interp` / `array_ops`. **Still intrinsic / private, or not linked**:
  the rest of `StringOps`, the rest of the numerics, the other mutable
  collections. `List.unapplySeq` is `SeqOps`'s identity in the library. The
  varargs `apply` of `List`/`Seq`/`LazyList`/`Array` is **library only**.
- **Library (what `agent/seqpat` added)**: `unapplySeq` on `Seq$` / `Vector$` /
  `IndexedSeq$` (an identity in practice; reads go through
  `lengthCompare$extension` / `apply$extension` / `drop$extension` on
  `scala/collection/SeqFactory$UnapplySeqWrapper$`) and `Array$.unapplySeq` (the
  same extensions on `scala/Array$UnapplySeqWrapper$`). `StringOps.map` becomes
  two methods: `Char => Char` goes to
  `map$extension(String, Function1)String` and everything else to
  `map$extension(String, Function1)IndexedSeq`. All of these are **library
  only**, and are diagnosed under `--no-scala-library`. Dual-run: `seqpat` /
  `seqpat_map` / `seqpat_ids` (`seqpat_ids` produces the same output on the
  private runtime too).
- **object**: as with scalac, a module `Main$` and a static forwarder `Main` are
  emitted. That is why `java Main` works.
- **Primitives**: `+` and friends on `Int` are emitted as JVM instructions
  (`iadd`, …), not as boxed methods on `scala.Int`.
- **traits**: a trait with only abstract members is a JVM interface. Concrete
  members become a `T$class` static implementation plus instance forwarders in
  C3 linearization order. Java 8 default methods are not used. A `val` becomes
  getter/setter plus `$init$`. `abstract override` becomes `T$$super$m`.
- **Named arguments**: reordered at the call site, so `f(b = 2, a = 1)` works.
  There is no large rewrite phase. Reordering happens for methods, `apply`,
  `copy`, constructors and calls with overloads alike, and omitted default
  arguments are filled in on the spot (through a `{method}$default$n` getter for
  ordinary methods; for constructors the default expression is typed at the call
  site). Extractor patterns are reordered too, for case classes. The parser
  parses `x = e` uniformly as an assignment, and **it is the typer that treats
  one in argument position as a named argument** (the same construction as nsc).
- **try**: an exception table and a `StackMapTable` are emitted in the `Code`
  attribute.
- **Lambdas**: a plain `FunctionN` literal becomes **`invokedynamic` +
  `LambdaMetafactory`**, as in nsc 2.13, and no class file is emitted. The body
  becomes a `public static final synthetic $anonfun$N` in the enclosing class
  file, and captured values are the call site's arguments. **A `{ case }` where a
  `PartialFunction` is expected, and a position expecting a user-defined SAM
  type, are still synthetic classes** (`Main$$$anonfun$0` and the like).
  `PartialFunction` has two abstract methods, so it is not a SAM, and nsc emits a
  class file here too. When a synthetic class is used, locals of the enclosing
  method are captured into `$captured$n` fields and **the enclosing `this`** into
  an `$outer` field, the same as nsc. `this` is needed not only when it is
  written explicitly (`this.f` / `super.f`) but equally when the lambda merely
  **calls a method of the enclosing class** (`xs.map(a => base(a))`); members of
  an `object` do not need it, since they go through `MODULE$`. See "compiling
  lambdas to `invokedynamic`" for the details.
- **Phases**: there are no separate passes like nsc's mixin. There are
  **uncurry**, **lambda-lift** (nested defs), erasure, and the closure conversion
  of lambdas.
- **sealed**: a non-exhaustive match is a warning, as in scalac. It becomes an
  error under `-Xfatal-warnings`.
- **AnyVal**: scalac emits both the value class's class file and the extension
  methods. scala-rs does the same: `new C(x)` erases to the underlying value and
  calls go to the `$extension` static methods. In positions that need a reference
  (`Any` / a universal trait / a type argument / an array element) it boxes with
  `new C(u)` as nsc does, and synthesises `equals` / `hashCode` from the
  underlying value. The difference is where the `$extension` bodies live: nsc puts
  them in the companion `C$` and makes the class side a forwarder, while scala-rs
  emits them directly on the class.
- **Predef / StringOps**: on the private runtime there are `assert` / `require` /
  `???` / `->` (straight to `Tuple2`) / `identity` / `locally` / `implicitly` /
  `any2stringadd`, and `length`/`toInt`/`isEmpty` on `String`. In library mode:
  `Predef$.println/assert/require/???/identity/locally/implicitly`,
  `any2stringadd.$plus$extension`, `ArrowAssoc.$minus$greater$extension`,
  `intWrapper` → `RichInt.abs$extension` / `max$extension` / `to$extension` /
  `until$extension`, `longWrapper` → `RichLong.abs$extension` / `max$extension` /
  `to` / `until` (`NumericRange$.inclusive` / `apply` +
  `Numeric$LongIsIntegral$`), `doubleWrapper` → `RichDouble.abs$extension` /
  `max$extension`, `floatWrapper` → `RichFloat.abs$extension` / `max$extension`,
  `charWrapper` → `RichChar.isDigit$extension` / `intValue$extension` (`.toInt`) /
  `to` / `until` (`NumericRange$.inclusive` / `apply` + `Numeric$CharIsIntegral$`),
  `byteWrapper` → `RichByte.abs$extension` / `max$extension` / `to` / `until`
  (`NumericRange$.inclusive` / `apply` + `Numeric$ByteIsIntegral$`),
  `shortWrapper` → `RichShort.max$extension` / `to` / `until`
  (`NumericRange$.inclusive` / `apply` + `Numeric$ShortIsIntegral$`),
  `booleanWrapper` → `RichBoolean.compare(Object)`, `augmentString` →
  `StringOps.toInt$extension` / `size$extension` (`.length`) /
  `$times$extension` / `take$extension` / `drop$extension` /
  `stripPrefix$extension` / `split$extension` / `stripSuffix$extension` /
  `padTo$extension` (`Int, Char`) / `linesIterator$extension` /
  `toIntOption$extension` / `stripMargin$extension` / `lines$extension` /
  `capitalize$extension` / `reverse$extension` / `slice$extension` /
  `takeRight$extension` / `dropRight$extension` / `contains$extension`
  (`.isEmpty` / `.toUpperCase` / `.toLowerCase` are inlined through `StringOps`
  to `String`; `startsWith` / `endsWith` / `indexOf` go to `java.lang.String` as
  in nsc; also `head$extension` / `last$extension` / `stripLineEnd$extension` /
  `replaceAllLiterally$extension` / `tail$extension` / `init$extension` /
  `distinct$extension` / `mkString$extension` / `filter$extension` /
  `reverseIterator$extension`). `intArrayOps` → `ArrayOps.head$extension` /
  `tail$extension` / `foreach$extension(Object,Function1)V` /
  `map$extension(Object,Function1,ClassTag)Object`. `longArrayOps` → the same
  `head` / `foreach` (`[J]`). `refArrayOps` → `map` on reference arrays. **No
  `StringOps` / `ArrayOps` / `RichInt` / `RichLong` / `RichDouble` / `RichFloat` /
  `RichChar` / `RichByte` / `RichShort` / `RichBoolean` / `ArrayBuffer` /
  `ListBuffer` / `StringBuilder` / `HashMap` / `HashSet` / `LinkedHashMap` /
  `LinkedHashSet` / `ArrayDeque` / `NumericRange` class files are emitted.**
- **unapplySeq**: `List` / `Seq` / `Vector` / `IndexedSeq` / `Array` and
  user-defined extractors, `_*`, and named case class patterns. When linked
  against the library, `List.unapplySeq` returns `SeqOps`, and everything other
  than `List` indexes through `UnapplySeqWrapper`'s `$extension`, as in nsc.
  Sequence patterns on `Seq` / `Array` require jar linking (they are diagnosed
  under `--no-scala-library`).

It is not a replacement for scalac. It is a reimplementation of a subset.
